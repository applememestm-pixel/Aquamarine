//! The server's data folder: accounts database, files, settings, journal.
//!
//! Every file of the server is a relative path ("aqua-directory.sqlite",
//! "aqua-blobs/" …), so picking the folder means entering it before
//! anything is opened. How it's found, first match wins:
//!
//! 1. `--data <folder>` or `AQUA_DATA=<folder>`;
//! 2. the folder chosen earlier (a one-line file in the user's config
//!    folder, `aquamarine/server-data.txt`);
//! 3. the current folder, if the server's data is already there (older
//!    setups keep working as they were);
//! 4. first start in a terminal: ask, with a sensible default;
//!    without a terminal (a service, Docker): the current folder.
//!
//! The admin panel shows the folder and can copy everything to another
//! one (it's used from the next start).

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Files that mean "a server already lives here".
const MARKERS: &[&str] = &["aqua-directory.sqlite", "aqua-blobs", "aqua-admin.json", "aqua-federation.json"];

fn config_file() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("aquamarine").join("server-data.txt"))
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Default suggestion: a visible folder in the home directory.
pub fn default_dir() -> PathBuf {
    home().map(|h| h.join("Aquamarine Server")).unwrap_or_else(|| PathBuf::from("aqua-data"))
}

pub fn has_data(dir: &Path) -> bool {
    MARKERS.iter().any(|m| dir.join(m).exists())
}

fn remembered() -> Option<PathBuf> {
    let f = config_file()?;
    let s = std::fs::read_to_string(f).ok()?;
    let p = PathBuf::from(s.trim());
    (!s.trim().is_empty()).then_some(p)
}

/// Remember `dir` for the next starts.
pub fn remember(dir: &Path) -> std::io::Result<()> {
    let Some(f) = config_file() else { return Ok(()) };
    if let Some(parent) = f.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(f, dir.display().to_string())
}

fn arg_dir() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--data" { return args.next().map(PathBuf::from); }
        if let Some(v) = a.strip_prefix("--data=") { return Some(PathBuf::from(v)); }
    }
    std::env::var_os("AQUA_DATA").filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn ask(cwd: &Path) -> PathBuf {
    let def = default_dir();
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("aqua-data")));
    println!();
    println!("  Where should the server keep its data?");
    println!("  ---------------------------------------");
    println!("  Accounts, encrypted files, settings and the journal go to one folder.");
    println!("  1) {}   ← Enter", def.display());
    if let Some(e) = &exe_dir { println!("  2) {}   (next to the program)", e.display()); }
    println!("  3) {}   (the current folder)", cwd.display());
    println!("  …or type a full path to any folder.");
    loop {
        print!("  Folder [1]: ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line).is_err() { return def; }
        let t = line.trim().trim_matches('"');
        let pick = match t {
            "" | "1" => def.clone(),
            "2" => match &exe_dir { Some(e) => e.clone(), None => continue },
            "3" => cwd.to_path_buf(),
            p if Path::new(p).is_absolute() => PathBuf::from(p),
            _ => { println!("  Type 1, 2, 3 or a full path (like {}).", def.display()); continue; }
        };
        match std::fs::create_dir_all(&pick) {
            Ok(()) => { println!("  Data folder: {}\n", pick.display()); return pick; }
            Err(e) => println!("  Can't use {}: {e}", pick.display()),
        }
    }
}

/// Find (or ask for) the data folder, create it and make it the current
/// folder. Returns it.
pub fn choose_and_enter() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (dir, save) = if let Some(d) = arg_dir() {
        (d, false)
    } else if let Some(d) = remembered() {
        (d, false)
    } else if has_data(&cwd) {
        (cwd.clone(), false)
    } else if std::io::stdin().is_terminal() && std::env::var("AQUA_DATA_PROMPT").map(|v| v != "0").unwrap_or(true) {
        (ask(&cwd), true)
    } else {
        (cwd.clone(), false)
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("  Can't create the data folder {}: {e} — using {}", dir.display(), cwd.display());
        return cwd;
    }
    if save { let _ = remember(&dir); }
    if let Err(e) = std::env::set_current_dir(&dir) {
        eprintln!("  Can't enter {}: {e}", dir.display());
        return cwd;
    }
    dir
}

/// The folder the server runs in now (its data).
pub fn current() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Total size of a folder.
pub fn size_of(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten().map(|e| match e.metadata() {
        Ok(m) if m.is_dir() => size_of(&e.path()),
        Ok(m) => m.len(),
        Err(_) => 0,
    }).sum()
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<u64> {
    std::fs::create_dir_all(to)?;
    let mut n = 0;
    for e in std::fs::read_dir(from)?.flatten() {
        let src = e.path();
        let dst = to.join(e.file_name());
        let md = e.metadata()?;
        if md.is_dir() {
            n += copy_tree(&src, &dst)?;
        } else if md.is_file() {
            std::fs::copy(&src, &dst)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Copy everything to `to` and use it from the next start. The old folder
/// stays as it is (delete it yourself once the server runs from the new one).
pub fn move_to(to: &Path) -> Result<u64, String> {
    if !to.is_absolute() { return Err("give a full path".into()); }
    let from = current();
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if canon(to) == canon(&from) { return Err("that is the current folder".into()); }
    if canon(to).starts_with(canon(&from)) { return Err("the new folder can't be inside the current one".into()); }
    if to.exists() && std::fs::read_dir(to).map(|mut r| r.next().is_some()).unwrap_or(false) && !has_data(to) {
        return Err("the folder isn't empty".into());
    }
    let n = copy_tree(&from, to).map_err(|e| e.to_string())?;
    remember(to).map_err(|e| e.to_string())?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copies_and_refuses_nesting() {
        let base = std::env::temp_dir().join(format!("aqua-dd-{}", std::process::id()));
        let a = base.join("a");
        std::fs::create_dir_all(a.join("aqua-blobs")).unwrap();
        std::fs::write(a.join("aqua-admin.json"), b"{}").unwrap();
        std::fs::write(a.join("aqua-blobs/x"), b"123").unwrap();
        assert!(has_data(&a));
        assert_eq!(copy_tree(&a, &base.join("b")).unwrap(), 2);
        assert!(base.join("b/aqua-blobs/x").exists());
        assert_eq!(size_of(&base.join("b")), 5);
        let _ = std::fs::remove_dir_all(&base);
    }
}
