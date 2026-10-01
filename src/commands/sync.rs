use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use anyhow::{Context, Result, bail};
use icoextract_rs::{IconExtractor, IconSelector};

use crate::{
    commands::run,
    config::{self, Effective, Game},
};

#[derive(PartialEq, Debug)]
pub enum SyncReport {
    Success(usize),
    WithFailure((usize, usize)),
}

pub fn render_desktop_entry(
    name: &str,
    eff: &Effective,
    is_game: bool,
    icopath: &Path,
) -> Result<String> {
    if !eff
        .app_entry
        .as_deref()
        .map(|s| Path::new(s).exists())
        .unwrap_or_default()
    {
        bail!("{name} does not have an valid app entry!");
    }
    let ico = extract_write_ico(
        eff.app_entry.as_deref().context("getting entry")?,
        name,
        icopath,
    );
    let ico = match ico {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("unable to extract icon for {name}: {e:#}");
            None
        }
    };

    let mut to_write = format!(
        r#"[Desktop Entry]
Type=Application
Name={name}
Comment=Open {name}
Exec=umuw run "{name}"
Terminal=false
"#
    );
    if is_game {
        to_write.push_str("Categories=Game;\n");
    }
    if let Some(icopath) = ico {
        to_write.push_str(&format!("Icon={}\n", icopath.to_string_lossy()));
    }
    if !eff.keywords.is_empty() {
        to_write.push_str(&format!("Keywords={};\n", eff.keywords.join(";")));
    }
    Ok(to_write)
}

pub fn sync(config_root: &Path, out_dir: &Path, icopath: &Path) -> Result<SyncReport> {
    fs::create_dir_all(icopath).context("creating icon dir for umuw")?;
    fs::create_dir_all(out_dir).context("creating applicaiton dir for umuw")?;
    let games = config::list_all_profile_toml::<Game>(config_root)?;
    let mut failures = vec![];
    let mut total = 0;

    for f in fs::read_dir(out_dir).with_context(|| format!("reading dir {}", out_dir.display()))? {
        let entry = f.context("obtaining file entry")?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .map(|ext| ext == "desktop")
                .unwrap_or_default()
        {
            fs::remove_file(entry.path()).with_context(|| {
                format!("removing stale entry: {}", entry.file_name().display())
            })?;
        }
    }

    for game_path in games {
        if let Some(name) = game_path.file_stem() {
            total += 1;
            let name = name.to_string_lossy().to_string();
            let eff = run::get_effective(config_root, &name, None, vec![])?;

            // set to true for now
            match render_desktop_entry(&name, &eff, true, icopath) {
                Err(e) => {
                    eprintln!("Unable to render the desktop entry for {name}: {e}");
                    failures.push(name);
                }
                Ok(s) => fs::write(out_dir.join(format!("{name}.desktop")), s)
                    .context("writing to .desktop")?,
            }
        }
    }

    if failures.is_empty() {
        Ok(SyncReport::Success(total))
    } else {
        Ok(SyncReport::WithFailure((total - failures.len(), total)))
    }
}

pub fn refresh_desktop_entries(location: &Path) {
    println!("Refreshing desktop database at {}", location.display());
    if let Err(e) = process::Command::new("update-desktop-database")
        .arg(location)
        .status()
    {
        eprintln!("failed to refresh desktop database, please refresh manually: {e}")
    }
}

fn extract_write_ico(entry: &str, name: &str, icopath: &Path) -> Result<PathBuf> {
    let write_path = icopath.join(name.replace(" ", "-") + ".ico");
    if write_path.exists() {
        return Ok(write_path);
    }
    let extractor: IconExtractor =
        IconExtractor::from_path(entry).with_context(|| format!("reading exe from {}", entry))?;
    extractor
        .export_icon(IconSelector::Index(0), &write_path)
        .with_context(|| format!("saving ico to {}", write_path.display()))?;
    Ok(write_path)
}

#[cfg(test)]
mod tests {
    use tempfile::{NamedTempFile, TempDir};

    use crate::config::SaveableToml;

    use super::*;

    #[test]
    fn sync_adds_entries() {
        let t_config = TempDir::new().unwrap();
        let t_icopath = TempDir::new().unwrap();
        let t_out = TempDir::new().unwrap();

        let e1 = NamedTempFile::new().unwrap();
        let e2 = NamedTempFile::new().unwrap();
        Game {
            app_entry: Some(e1.path().to_string_lossy().to_string()),
            ..Default::default()
        }
        .save(t_config.path(), "Game test 1")
        .unwrap();

        Game {
            app_entry: Some(e2.path().to_string_lossy().to_string()),
            ..Default::default()
        }
        .save(t_config.path(), "Game test 2")
        .unwrap();
        let res = sync(t_config.path(), t_out.path(), t_icopath.path()).unwrap();
        assert_eq!(res, SyncReport::Success(2));

        let content = fs::read_to_string(t_out.path().join("Game test 1.desktop")).unwrap();
        assert!(content.contains(r#"Exec=umuw run "Game test 1""#));
        assert!(content.contains("Categories=Game;"));
        assert!(content.contains("Terminal=false"));
    }

    #[test]
    fn sync_avoids_invalid_entry() {
        let t_config = TempDir::new().unwrap();
        let t_icopath = TempDir::new().unwrap();
        let t_out = TempDir::new().unwrap();

        let e1 = NamedTempFile::new().unwrap();
        Game {
            app_entry: Some(e1.path().to_string_lossy().to_string()),
            ..Default::default()
        }
        .save(t_config.path(), "Game test 1")
        .unwrap();

        Game {
            app_entry: Some("/non/existant/entry".into()),
            ..Default::default()
        }
        .save(t_config.path(), "Game test 2")
        .unwrap();
        let res = sync(t_config.path(), t_out.path(), t_icopath.path()).unwrap();
        assert_eq!(res, SyncReport::WithFailure((1, 2)));
    }

    #[test]
    fn sync_removes_stale_entries() {
        let t_config = TempDir::new().unwrap();
        let t_icopath = TempDir::new().unwrap();
        let t_out = TempDir::new().unwrap();

        let e1 = NamedTempFile::new().unwrap();
        let e2 = NamedTempFile::new().unwrap();
        let stale = NamedTempFile::new_in(&t_out).unwrap();
        Game {
            app_entry: Some(e1.path().to_string_lossy().to_string()),
            ..Default::default()
        }
        .save(t_config.path(), "Game test 1")
        .unwrap();

        Game {
            app_entry: Some(e2.path().to_string_lossy().to_string()),
            ..Default::default()
        }
        .save(t_config.path(), "Game test 2")
        .unwrap();
        let res = sync(t_config.path(), t_out.path(), t_icopath.path()).unwrap();
        assert_eq!(res, SyncReport::Success(2));
        assert!(stale.path().exists());

        for f in fs::read_dir(t_config.path().join(Game::SUBDIR)).unwrap() {
            let f = f.unwrap();
            fs::remove_file(f.path()).unwrap();
        }
        let res = sync(t_config.path(), t_out.path(), t_icopath.path()).unwrap();
        assert!(stale.path().exists());
        assert_eq!(res, SyncReport::Success(0));
        assert!(fs::read_dir(&t_out).unwrap().collect::<Vec<_>>().len() == 1);
    }

    #[test]
    fn sync_is_idempotent() {
        let t_config = TempDir::new().unwrap();
        let t_icopath = TempDir::new().unwrap();
        let t_out = TempDir::new().unwrap();

        let e1 = NamedTempFile::new().unwrap();
        let e2 = NamedTempFile::new().unwrap();
        Game {
            app_entry: Some(e1.path().to_string_lossy().to_string()),
            ..Default::default()
        }
        .save(t_config.path(), "Game test 1")
        .unwrap();

        Game {
            app_entry: Some(e2.path().to_string_lossy().to_string()),
            ..Default::default()
        }
        .save(t_config.path(), "Game test 2")
        .unwrap();
        let res1 = sync(t_config.path(), t_out.path(), t_icopath.path()).unwrap();
        let r1 = fs::read_dir(&t_out)
            .unwrap()
            .map(|e| e.unwrap().path())
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>();
        let res2 = sync(t_config.path(), t_out.path(), t_icopath.path()).unwrap();
        let r2 = fs::read_dir(&t_out)
            .unwrap()
            .map(|e| e.unwrap().path())
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(res1, SyncReport::Success(2));
        assert_eq!(res1, res2);
        assert_eq!(r1, r2);
    }
}
