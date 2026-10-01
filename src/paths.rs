use anyhow::{Context, Result, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use tabular::{Table, row, table};

use crate::config::{Game, Group, SaveableToml, Seeded};

pub enum ProtonPathOrigin {
    HomeRelative(String),
    Absolute(String),
}

impl ProtonPathOrigin {
    pub fn to_path_buf(&self) -> Result<PathBuf> {
        match self {
            ProtonPathOrigin::HomeRelative(path) => Ok(get_home()?.join(path)),
            ProtonPathOrigin::Absolute(path) => Ok(PathBuf::from(path)),
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            ProtonPathOrigin::HomeRelative(path) => format!("~/{}", path),
            ProtonPathOrigin::Absolute(path) => path.to_string(),
        }
    }
}

pub fn absolutize_path(path: &str) -> Result<PathBuf> {
    let expanded_path = shellexpand::tilde(path);
    Ok(env::current_dir()
        .context("obtaining cwd")?
        .join(expanded_path.as_ref()))
}

pub fn get_proton_origins() -> Vec<ProtonPathOrigin> {
    vec![
        ProtonPathOrigin::HomeRelative(".local/share/Steam/compatibilitytools.d".to_owned()),
        ProtonPathOrigin::Absolute("/usr/share/steam/compatibilitytools.d/".to_owned()),
    ]
}

pub fn obtain_proton_path(
    proton_name: &str,
    proton_path_list: &[ProtonPathOrigin],
) -> Result<PathBuf> {
    for proton_folder in proton_path_list {
        let proton_location = match proton_folder.to_path_buf() {
            Ok(path) => path.join(proton_name),
            Err(e) => {
                eprintln!(
                    "warning unable to access: {}: {e:#}",
                    proton_folder.as_str()
                );
                continue;
            }
        };

        if proton_location.exists() {
            return Ok(proton_location);
        }
    }
    bail!(
        "Unable to find {}. Available runners: \n{}",
        proton_name,
        obtain_proton_list(proton_path_list)?
    );
}

pub fn initialize_config(config_root: &Path) -> Result<()> {
    ensure_config(config_root)?;
    let grp_folder = Group::ensure_config_folder(config_root)?;
    Game::ensure_config_folder(config_root)?;
    // check if default group exist.
    if !fs::exists(grp_folder.join("default.toml"))
        .with_context(|| "Unable to access default group config")?
    {
        Group::seed().save(config_root, "default")?;
    }
    Ok(())
}

pub fn get_home() -> Result<PathBuf> {
    Ok(directories::BaseDirs::new()
        .context("Obtaining $Home")?
        .home_dir()
        .to_path_buf())
}

pub fn edit(path: &str, editor: Option<&str>) -> Result<Option<i32>> {
    if !Path::new(path).exists() {
        bail!("{path} does not exist!")
    }
    let default_editor = env::var("EDITOR")
        .or_else(|_| env::var("VISUAL"))
        .context("obtaining default editor using $EDITOR/$VISUAL")?;
    let mut args = editor
        .map(|s| s.split_whitespace().collect::<Vec<&str>>())
        .unwrap_or_else(|| vec![&default_editor]);
    args.push(path);
    let editor = args.remove(0);
    let mut cmd = std::process::Command::new(editor);
    cmd.args(args)
        .status()
        .map(|res| res.code())
        .with_context(|| format!("editing {path} using {editor}"))
}

fn obtain_proton_list(proton_path_list: &[ProtonPathOrigin]) -> Result<Table> {
    let mut ret = table!("{:<}    {:<}", row!("NAME", "LOCATION"));
    for proton_path in proton_path_list {
        let Ok(path) = proton_path.to_path_buf() else {
            continue;
        };
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };
        for f in entries.flatten() {
            if let Ok(folder) = f.file_type()
                && folder.is_dir()
            {
                ret.add_row(row!(f.file_name().to_string_lossy(), path.display()));
            }
        }
    }
    Ok(ret)
}

fn ensure_config(config_root: &Path) -> Result<()> {
    // creates the project config directory and returns its directory path
    if !config_root.exists() {
        fs::create_dir_all(config_root)
            .with_context(|| format!("Creating config dir: {}", config_root.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;

    use tempfile::TempDir;

    use crate::config::load_profile;

    use super::*;

    #[test]
    fn absolutize_path_ignores_if_path_is_already_absolute() {
        let p1 = "/some/folder/that/is/absolute";
        let p2 = "~/some/folder/that/is/home";
        let p2_expected = get_home().unwrap().join("some/folder/that/is/home");
        assert_eq!(p1, absolutize_path(p1).unwrap().to_string_lossy());
        assert_eq!(
            p2_expected,
            absolutize_path(p2).unwrap().to_string_lossy().to_string()
        );
    }

    #[test]
    fn absolutize_path_converts_cwd_to_absolute() {
        let p = "./some/folder/app";
        let expected = env::current_dir().unwrap().join(p);
        assert_eq!(
            expected,
            absolutize_path(p).unwrap().to_string_lossy().to_string()
        );
    }

    #[test]
    fn initialize_config_creates_default_group_profile() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_dir.path();
        let default_path = Group::dir(root).join("default.toml");
        assert!(!default_path.exists());
        initialize_config(root).unwrap();
        assert!(default_path.exists());
        let saved = load_profile::<Group>(&default_path).unwrap();
        assert_eq!(saved, Group::seed());
    }

    #[test]
    fn initialize_config_is_idempotent() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_dir.path();
        let default_path = Group::dir(root).join("default.toml");
        assert!(!default_path.exists());
        initialize_config(root).unwrap();
        let before = fs::read(&default_path).unwrap();
        initialize_config(root).unwrap();
        assert_eq!(before, fs::read(&default_path).unwrap());
    }

    #[test]
    fn initialize_config_is_non_destructive() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_dir.path();
        let default_path = Group::dir(root).join("default.toml");
        assert!(!default_path.exists());
        initialize_config(root).unwrap();
        let modified_group = Group {
            runner: Some("test-proton".to_owned()),
            ..Group::seed()
        };
        modified_group.save(root, "default").unwrap();
        let saved = load_profile::<Group>(&default_path).unwrap();
        assert_eq!(saved, modified_group);

        initialize_config(root).unwrap();
        let saved = load_profile::<Group>(&default_path).unwrap();
        assert_eq!(saved, modified_group);
    }

    #[test]
    fn initialize_config_repairs_partial_state() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_dir.path();
        let game_folder = Game::dir(root);
        assert!(!game_folder.exists());

        initialize_config(root).unwrap();

        // check integrity of other files
        let default_path = Group::dir(root).join("default.toml");
        let modified_group = Group {
            runner: Some("test-proton".to_owned()),
            ..Group::seed()
        };
        modified_group.save(root, "default").unwrap();
        let saved = load_profile::<Group>(&default_path).unwrap();
        assert_eq!(saved, modified_group);

        assert!(game_folder.exists());
        fs::remove_dir(&game_folder).unwrap();
        assert!(!game_folder.exists());
        initialize_config(root).unwrap();
        assert!(game_folder.exists());

        // ensure other files is not changed
        let saved = load_profile::<Group>(&default_path).unwrap();
        assert_eq!(saved, modified_group);
    }

    #[test]
    fn get_home_gets_user_home() {
        let res = get_home().unwrap();
        assert_eq!(res.to_str().unwrap(), env::var("HOME").unwrap());
    }

    #[test]
    fn obtain_proton_path_gets_correct_proton_location() {
        let proton_name = "example-proton latest";
        let proton_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let proton_dir_path = proton_dir.path();
        let proton_location = proton_dir_path.join(proton_name);
        fs::create_dir(&proton_location).unwrap();
        assert!(proton_location.exists());

        let proton_search_dir = &[ProtonPathOrigin::Absolute(
            proton_dir_path.to_string_lossy().to_string(),
        )];

        let res = obtain_proton_path(proton_name, proton_search_dir).unwrap();
        assert_eq!(res, proton_location)
    }
}
