//! `airmicd install` and `airmicd uninstall`: the systemd user unit for the running binary.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

const UNIT_TEMPLATE: &str = include_str!("../../../../packaging/airmicd.service");
const UNIT_FILE: &str = "airmicd.service";

type Systemctl<'a> = &'a mut dyn FnMut(&[&str]) -> anyhow::Result<()>;

/// Writes, enables and starts the user unit. `dry_run` only prints what it would do.
pub fn install(dry_run: bool) -> anyhow::Result<()> {
    refuse_sudo()?;
    let exe = std::env::current_exe()
        .and_then(fs::canonicalize)
        .context("finding the airmicd binary")?;
    let unit = render_unit(&exe)?;
    let dir = find_user_unit_dir()?;
    if dry_run {
        println!("Would write {}:\n\n{unit}", dir.join(UNIT_FILE).display());
        println!(
            "Then run: systemctl --user daemon-reload && systemctl --user enable --now airmicd"
        );
        return Ok(());
    }
    install_unit(&dir, &unit, &mut run_systemctl)
}

/// Stops, disables and removes the user unit.
pub fn uninstall() -> anyhow::Result<()> {
    refuse_sudo()?;
    uninstall_unit(&find_user_unit_dir()?, &mut run_systemctl)
}

fn install_unit(dir: &Path, unit: &str, systemctl: Systemctl) -> anyhow::Result<()> {
    let path = dir.join(UNIT_FILE);
    let previous = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let changed = previous.as_deref() != Some(unit);
    if changed {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        fs::write(&path, unit).with_context(|| format!("writing {}", path.display()))?;
    }
    match &previous {
        None => println!("Wrote {}", path.display()),
        Some(_) if !changed => println!("{} is already up to date", path.display()),
        Some(old) => println!(
            "Replaced {}, which ran {}",
            path.display(),
            find_exec_start(old).unwrap_or("something else")
        ),
    }
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", "airmicd"])?;
    // enable --now leaves an already running daemon on the old unit.
    if previous.is_some() && changed {
        systemctl(&["restart", "airmicd"])?;
    }
    println!("airmicd is running and starts on login.");
    println!("Next: run `airmicd pair` and enter the code on your iPhone.");
    Ok(())
}

fn uninstall_unit(dir: &Path, systemctl: Systemctl) -> anyhow::Result<()> {
    let path = dir.join(UNIT_FILE);
    if !path.exists() {
        println!(
            "airmicd is not installed ({} does not exist)",
            path.display()
        );
        return Ok(());
    }
    systemctl(&["disable", "--now", "airmicd"])?;
    fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    systemctl(&["daemon-reload"])?;
    println!("Stopped airmicd and removed {}", path.display());
    Ok(())
}

/// Returns the packaged unit with `ExecStart` pointing at `exe`.
fn render_unit(exe: &Path) -> anyhow::Result<String> {
    let exe = exe
        .to_str()
        .context("the airmicd path is not valid UTF-8")?;
    // systemd expands `%` specifiers and splits the command line on whitespace.
    let mut exe = exe.replace('%', "%%");
    if exe.contains(|c: char| c.is_whitespace() || c == '"' || c == '\\') {
        exe = format!("\"{}\"", exe.replace('\\', "\\\\").replace('"', "\\\""));
    }
    Ok(UNIT_TEMPLATE
        .lines()
        .map(|line| match line.starts_with("ExecStart=") {
            true => format!("ExecStart={exe}\n"),
            false => format!("{line}\n"),
        })
        .collect())
}

fn find_exec_start(unit: &str) -> Option<&str> {
    unit.lines()
        .find_map(|line| line.strip_prefix("ExecStart="))
}

fn find_user_unit_dir() -> anyhow::Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().join("systemd/user"))
        .context("no home directory")
}

/// Under sudo the unit would land in root's systemd, not the user's.
fn refuse_sudo() -> anyhow::Result<()> {
    if std::env::var_os("SUDO_USER").is_some() {
        bail!("run this without sudo: airmicd runs as a systemd user service");
    }
    Ok(())
}

fn run_systemctl(args: &[&str]) -> anyhow::Result<()> {
    let output = match Command::new("systemctl").arg("--user").args(args).output() {
        Ok(output) => output,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            bail!("systemctl not found: airmicd install needs systemd")
        }
        Err(e) => return Err(e).context("running systemctl"),
    };
    if !output.status.success() {
        bail!(
            "`systemctl --user {}` failed: {}\nIs a systemd user session running? \
             It needs a graphical or local login, not a bare ssh or su shell.",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(calls: &mut Vec<String>) -> impl FnMut(&[&str]) -> anyhow::Result<()> + '_ {
        |args| {
            calls.push(args.join(" "));
            Ok(())
        }
    }

    #[test]
    fn template_has_one_exec_start() {
        let count = UNIT_TEMPLATE
            .lines()
            .filter(|l| l.starts_with("ExecStart="))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn render_points_exec_start_at_the_binary() {
        let unit = render_unit(Path::new("/home/me/airmic/airmicd")).unwrap();
        assert_eq!(find_exec_start(&unit), Some("/home/me/airmic/airmicd"));
        assert!(unit.contains("WantedBy=default.target\n"));
        assert!(!unit.contains("/usr/bin/airmicd"));
    }

    #[test]
    fn render_quotes_spaces_and_escapes_specifiers() {
        let unit = render_unit(Path::new("/home/me/my apps/100%/airmicd")).unwrap();
        assert_eq!(
            find_exec_start(&unit),
            Some("\"/home/me/my apps/100%%/airmicd\"")
        );
    }

    #[test]
    fn install_writes_unit_and_enables_it() {
        let dir = tempfile::tempdir().unwrap();
        let unit_dir = dir.path().join("systemd/user");
        let mut calls = Vec::new();
        install_unit(&unit_dir, "unit\n", &mut record(&mut calls)).unwrap();
        assert_eq!(
            fs::read_to_string(unit_dir.join(UNIT_FILE)).unwrap(),
            "unit\n"
        );
        assert_eq!(calls, ["daemon-reload", "enable --now airmicd"]);
    }

    #[test]
    fn install_again_does_not_restart() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(UNIT_FILE), "unit\n").unwrap();
        let mut calls = Vec::new();
        install_unit(dir.path(), "unit\n", &mut record(&mut calls)).unwrap();
        assert_eq!(calls, ["daemon-reload", "enable --now airmicd"]);
    }

    #[test]
    fn install_over_another_binary_replaces_and_restarts() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(UNIT_FILE), "ExecStart=/old/airmicd\n").unwrap();
        let mut calls = Vec::new();
        install_unit(
            dir.path(),
            "ExecStart=/new/airmicd\n",
            &mut record(&mut calls),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(UNIT_FILE)).unwrap(),
            "ExecStart=/new/airmicd\n"
        );
        assert_eq!(
            calls,
            ["daemon-reload", "enable --now airmicd", "restart airmicd"]
        );
    }

    #[test]
    fn install_reports_systemctl_failure() {
        let dir = tempfile::tempdir().unwrap();
        let mut fail = |_: &[&str]| bail!("no user session");
        let err = install_unit(dir.path(), "unit\n", &mut fail).unwrap_err();
        assert!(err.to_string().contains("no user session"));
    }

    #[test]
    fn uninstall_disables_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(UNIT_FILE), "unit\n").unwrap();
        let mut calls = Vec::new();
        uninstall_unit(dir.path(), &mut record(&mut calls)).unwrap();
        assert!(!dir.path().join(UNIT_FILE).exists());
        assert_eq!(calls, ["disable --now airmicd", "daemon-reload"]);
    }

    #[test]
    fn uninstall_when_not_installed_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut calls = Vec::new();
        uninstall_unit(dir.path(), &mut record(&mut calls)).unwrap();
        assert!(calls.is_empty());
    }
}
