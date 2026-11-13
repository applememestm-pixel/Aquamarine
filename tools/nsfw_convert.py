#!/usr/bin/env python3
"""Turn NSFWJS's MobileNetV2 (MIT, Infinite Red — npm `nsfwjs`, Keras /
TF.js layers model) into `crates/aqua-nsfw/model/aqnsfw.bin`, the compact
format the small Rust engine there runs (no TensorFlow / ONNX anywhere).

  python3 tools/nsfw_convert.py <model.json> <weights.bin> <out.bin> [--check img…]

model.json / weights.bin: the TF.js files of `dist/models/mobilenet_v2`
(the npm package wraps them in JS: the object literal and a base64 string).

What it does: ZeroPadding goes into the next convolution's padding,
BatchNormalization becomes a per-channel factor and bias of the convolution
before it, Dropout is dropped. The weights stay the model's own uint8 values
(~2.6 MB) — no accuracy is lost. `--check` runs the original float network and the
converted one in numpy on images and prints both — they must agree.

Format (little-endian): b"AQN1", u32 op count, then ops:
  1 conv:  u8 k, u8 stride, u8 pad t/l/b/r, u16 cin, u16 cout, u8 relu6,
           f32 lo, f32 scale, f32 bn[cout], u8 w[k][k][cin][cout], f32 bias[cout]
  2 dw:    u8 k, u8 stride, u8 pad t/l/b/r, u16 c, u8 relu6,
           f32 lo, f32 scale, f32 bn[c], u8 w[k][k][c], f32 bias[c]
  3 save:  u8 slot          (keep the current tensor)
  4 add:   u8 slot          (current += saved)
  5 gap                      (global average pool)
  6 dense: u16 in, u16 out, u8 act (0 none, 1 relu, 2 softmax),
           f32 lo, f32 scale, u8 w[in][out], f32 bias[out]
  Weights: (lo + q * scale) * bn[channel] — the model's own uint8 weights,
  untouched (no second quantization), BatchNorm folded in at load.
"""
import json, struct, sys
import numpy as np


def load(model_json, weights_bin):
    d = json.load(open(model_json))
    raw = open(weights_bin, "rb").read()
    W, Q, off = {}, {}, 0
    for w in d["weightsManifest"][0]["weights"]:
        n = int(np.prod(w["shape"])) if w["shape"] else 1
        q = w.get("quantization")
        if q:
            a = np.frombuffer(raw, dtype=np.uint8 if q["dtype"] == "uint8" else np.uint16, count=n, offset=off)
            off += n * a.itemsize
            assert a.dtype == np.uint8
            Q[w["name"]] = (a.reshape(w["shape"]), q["min"], q["scale"])
            a = a.astype(np.float64) * q["scale"] + q["min"]
        else:
            a = np.frombuffer(raw, dtype=np.float32, count=n, offset=off).astype(np.float64)
            off += n * 4
        W[w["name"]] = a.reshape(w["shape"])
    layers = d["modelTopology"]["model_config"]["config"]["layers"]
    return layers, W, Q


def same_pad(k):
    return ((k - 1) // 2, (k - 1) // 2, k // 2, k // 2)


def program(layers, W, Q):
    """Float ops in order: (kind, params…); BN folded, padding merged."""
    ops, pending_pad = [], None
    by_name = {l["name"]: l for l in layers}
    consumers = {}
    for l in layers:
        for n in (l["inbound_nodes"][0] if l["inbound_nodes"] else []):
            consumers.setdefault(n[0], []).append(l["name"])
    # Residual adds: the block input must be kept until its Add.
    add_inputs = {}
    for l in layers:
        if l["class_name"] == "Add":
            ins = [n[0] for n in l["inbound_nodes"][0]]
            add_inputs[l["name"]] = ins
    saved = {}  # tensor name -> slot
    for l in layers:
        for ins in add_inputs.values():
            pass
    slot_of = {}
    for name, ins in add_inputs.items():
        # the input that isn't the immediate predecessor in layer order is the skip
        slot_of[name] = ins
    order = [l["name"] for l in layers]
    skip_tensors = {}
    for name, ins in add_inputs.items():
        idx = order.index(name)
        prev = order[idx - 1]
        skip = ins[0] if ins[1] == prev else ins[1]
        skip_tensors[skip] = 0  # one slot: each skip is used by the very next Add
    add_slot = {name: skip_tensors[(ins[0] if ins[1] == order[order.index(name) - 1] else ins[1])] for name, ins in add_inputs.items()}
    i = 0
    while i < len(layers):
        l = layers[i]; c = l["config"]; cls = l["class_name"]; nm = l["name"]
        if cls == "InputLayer":
            pass
        elif cls == "ZeroPadding2D":
            (t, b), (lft, r) = c["padding"]
            pending_pad = (t, lft, b, r)
        elif cls in ("Conv2D", "DepthwiseConv2D"):
            k, s = c["kernel_size"][0], c["strides"][0]
            if pending_pad is not None:
                pad = pending_pad; pending_pad = None
            else:
                pad = same_pad(k) if c["padding"] == "same" else (0, 0, 0, 0)
            if cls == "Conv2D":
                q = Q[nm + "/kernel"]                      # k,k,cin,cout
            else:
                q0 = Q[nm + "/depthwise_kernel"]
                q = (q0[0][:, :, :, 0], q0[1], q0[2])     # k,k,c
            cout = q[0].shape[-1]
            bias = W.get(nm + "/bias", np.zeros(cout))
            f = np.ones(cout)
            relu = False
            nxt = layers[i + 1] if i + 1 < len(layers) else None
            if nxt and nxt["class_name"] == "BatchNormalization":
                bn = nxt["name"]; eps = nxt["config"]["epsilon"]
                g, be, mu, var = (W[bn + "/" + x] for x in ("gamma", "beta", "moving_mean", "moving_variance"))
                f = g / np.sqrt(var + eps)
                bias = (bias - mu) * f + be
                i += 1
                nxt = layers[i + 1] if i + 1 < len(layers) else None
            if nxt and nxt["class_name"] == "ReLU":
                assert nxt["config"].get("max_value") == 6
                relu = True; i += 1
            ops.append(("conv" if cls == "Conv2D" else "dw", k, s, pad, q, f, bias, relu))
            out_name = layers[i]["name"]
            if out_name in skip_tensors:
                ops.append(("save", skip_tensors[out_name]))
        elif cls == "Add":
            ops.append(("add", add_slot[nm]))
            if nm in skip_tensors:
                ops.append(("save", skip_tensors[nm]))
        elif cls == "AveragePooling2D":
            ops.append(("gap",))
        elif cls in ("Flatten", "Dropout"):
            pass
        elif cls == "Dense":
            act = {"linear": 0, "relu": 1, "softmax": 2}[c["activation"]]
            ops.append(("dense", Q[nm + "/kernel"], W[nm + "/bias"], act))
        elif cls == "BatchNormalization":
            raise SystemExit(f"unfolded BN {nm}")
        else:
            raise SystemExit(f"unsupported layer {cls} {nm}")
        i += 1
    return ops


def deq(q, lo, scale, f=None):
    """What the engine computes at load: (lo + q * scale) * f[channel]."""
    w = np.float32(lo) + q.astype(np.float32) * np.float32(scale)
    return (w * f.astype(np.float32) if f is not None else w).astype(np.float64)


def write(ops, path):
    out = [b"AQN1", struct.pack("<I", len(ops))]
    qops = []
    for op in ops:
        kind = op[0]
        if kind in ("conv", "dw"):
            _, k, s, pad, (q, lo, scale), f, bias, relu = op
            if kind == "conv":
                out.append(struct.pack("<BBBBBBBHHB", 1, k, s, *pad, q.shape[2], q.shape[3], int(relu)))
            else:
                out.append(struct.pack("<BBBBBBBHB", 2, k, s, *pad, q.shape[2], int(relu)))
            out += [struct.pack("<ff", lo, scale), f.astype(np.float32).tobytes(), np.ascontiguousarray(q).tobytes(), bias.astype(np.float32).tobytes()]
            qops.append((kind, k, s, pad, deq(q, lo, scale, f), bias.astype(np.float32).astype(np.float64), relu))
        elif kind == "save":
            out.append(struct.pack("<BB", 3, op[1])); qops.append(op)
        elif kind == "add":
            out.append(struct.pack("<BB", 4, op[1])); qops.append(op)
        elif kind == "gap":
            out.append(struct.pack("<B", 5)); qops.append(op)
        elif kind == "dense":
            _, (q, lo, scale), b, act = op
            out.append(struct.pack("<BHHB", 6, q.shape[0], q.shape[1], act))
            out += [struct.pack("<ff", lo, scale), np.ascontiguousarray(q).tobytes(), b.astype(np.float32).tobytes()]
            qops.append(("dense", deq(q, lo, scale), b.astype(np.float32).astype(np.float64), act))
    open(path, "wb").write(b"".join(out))
    return qops


def run(ops, x):
    """numpy forward (NHWC without N)."""
    slots = {}
    for op in ops:
        kind = op[0]
        if kind in ("conv", "dw"):
            _, k, s, (t, l, b, r), w, bias, relu = op
            x = np.pad(x, ((t, b), (l, r), (0, 0)))
            H = (x.shape[0] - k) // s + 1; Wd = (x.shape[1] - k) // s + 1
            if kind == "conv":
                y = np.zeros((H, Wd, w.shape[3]))
                for i in range(k):
                    for j in range(k):
                        y += x[i:i + s * H:s, j:j + s * Wd:s, :] @ w[i, j]
            else:
                y = np.zeros((H, Wd, w.shape[2]))
                for i in range(k):
                    for j in range(k):
                        y += x[i:i + s * H:s, j:j + s * Wd:s, :] * w[i, j]
            y += bias
            x = np.clip(y, 0, 6) if relu else y
        elif kind == "save":
            slots[op[1]] = x
        elif kind == "add":
            x = x + slots[op[1]]
        elif kind == "gap":
            x = x.mean(axis=(0, 1))
        elif kind == "dense":
            _, w, b, act = op
            x = x @ w + b
            if act == 1: x = np.maximum(x, 0)
            if act == 2: e = np.exp(x - x.max()); x = e / e.sum()
    return x


def image(path):
    from PIL import Image
    im = Image.open(path).convert("RGB").resize((224, 224), Image.BILINEAR)
    return np.asarray(im, dtype=np.float64) / 255.0


if __name__ == "__main__":
    layers, W, Q = load(sys.argv[1], sys.argv[2])
    ops = program(layers, W, Q)
    qops = write(ops, sys.argv[3])
    # The original network in float64 (unfolded math, same weights).
    ops = [(o[0], o[1], o[2], o[3], deq(*o[4]).astype(np.float64) * o[5], o[6], o[7]) if o[0] in ("conv", "dw")
           else ("dense", deq(*o[1]), o[2], o[3]) if o[0] == "dense" else o for o in ops]
    print(f"{sys.argv[3]}: {len(ops)} ops")
    if "--pattern" in sys.argv:
        # The input crates/aqua-nsfw's test feeds the engine (`pattern()`).
        y, x, c = np.meshgrid(np.arange(224), np.arange(224), np.arange(3), indexing="ij")
        p = ((x * 7 + y * 13 + c * 61) % 256).astype(np.float32).astype(np.float64) / 255.0
        print("REFERENCE:", ", ".join(f"{v:.6}" for v in run(qops, p)))
    if "--check" in sys.argv:
        names = ["Drawing", "Hentai", "Neutral", "Porn", "Sexy"]
        for p in sys.argv[sys.argv.index("--check") + 1:]:
            x = image(p)
            a, b = run(ops, x), run(qops, x)
            print(p.split("/")[-1], " ".join(f"{n}={v:.3f}/{w:.3f}" for n, v, w in zip(names, a, b)))
