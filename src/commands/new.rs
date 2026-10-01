use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{
    commands::new::ConfigStatus::{GroupAlrExist, GroupNotFound, Success},
    config::{self, Game, Group, SaveableToml},
    paths,
};

#[derive(Debug, PartialEq)]
pub enum ConfigStatus<'a> {
    Success(String),
    AppExist(String),
    InvalidPath(PathBuf),
    GroupAlrExist(&'a str),
    GroupNotFound(&'a str),
}

pub fn config<'a>(
    config_root: &Path,
    app_entry: String,
    group: Option<(&'a str, bool)>,
    name: Option<String>,
) -> Result<ConfigStatus<'a>> {
    // Goal: config consumes group, name to create a new Game and save it under config root
    // sanity checks
    let entry_path = paths::absolutize_path(&app_entry)?;
    if !entry_path.exists() || !entry_path.is_file() {
        return Ok(ConfigStatus::InvalidPath(entry_path));
    }
    let app_name = match name {
        Some(name) => name,
        None => entry_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .context("Obtaining file stem")?
            .to_owned(),
    };

    if Game::toml_path(config_root, &app_name).exists() {
        return Ok(ConfigStatus::AppExist(app_name));
    }

    let group = match group {
        Some((grp, new)) => match Group::ensure(config_root, grp, new)? {
            config::EnsureOutcome::AlrExist(name) => return Ok(GroupAlrExist(name)),
            config::EnsureOutcome::NotFound(name) => return Ok(GroupNotFound(name)),
            config::EnsureOutcome::Ensured(name) => Some(name.to_owned()),
        },
        None => None,
    };

    let new_app = Game {
        app_entry: Some(entry_path.to_string_lossy().to_string()),
        group,
        ..Default::default()
    };
    new_app.save(config_root, &app_name)?;
    Ok(Success(app_name))
}

#[cfg(test)]
mod tests {
    use tempfile::{NamedTempFile, TempDir};

    use super::*;
    use crate::commands::run::tests_util::TestCtx;
    use crate::dmenu::tests_util::FirstChoiceAdapter;

    #[test]
    fn config_creates_seeded_group_when_missing() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        let f = NamedTempFile::new().unwrap();
        let appname = f.path().file_stem().unwrap().to_string_lossy().to_string();
        let res = config(
            ctx.config_root,
            f.path().to_string_lossy().to_string(),
            Some(("unknown group", true)),
            None,
        )
        .unwrap();
        assert_eq!(res, ConfigStatus::Success(appname));
        assert!(Group::toml_path(ctx.config_root, "unknown group").exists());
    }

    #[test]
    fn config_refuses_when_group_exists() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        let f = NamedTempFile::new().unwrap();
        Group::default().save(ctx.config_root, "somegrp").unwrap();
        let res = config(
            ctx.config_root,
            f.path().to_string_lossy().to_string(),
            Some(("somegrp", true)),
            None,
        )
        .unwrap();
        assert_eq!(res, ConfigStatus::GroupAlrExist("somegrp"));
    }

    #[test]
    fn config_refuses_when_group_not_found() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        let f = NamedTempFile::new().unwrap();
        let res = config(
            ctx.config_root,
            f.path().to_string_lossy().to_string(),
            Some(("somegrp", false)),
            None,
        )
        .unwrap();
        assert_eq!(res, ConfigStatus::GroupNotFound("somegrp"));
    }

    #[test]
    fn config_refuses_existing_game() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        let f = NamedTempFile::new().unwrap();
        let appname = f.path().file_stem().unwrap().to_string_lossy().to_string();
        Game::default().save(ctx.config_root, &appname).unwrap();
        let res = config(
            ctx.config_root,
            f.path().to_string_lossy().to_string(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(res, ConfigStatus::AppExist(appname));
    }

    #[test]
    fn config_refuses_invalid_path() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        let f = "/some/non/existant/path".to_string();
        let res = config(ctx.config_root, f.to_string(), None, None).unwrap();
        assert_eq!(res, ConfigStatus::InvalidPath(PathBuf::from(f)));
    }

    #[test]
    fn config_refuses_non_file_appname() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        let temp_f = TempDir::new().unwrap();
        let res = config(
            ctx.config_root,
            temp_f.path().to_string_lossy().to_string(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(res, ConfigStatus::InvalidPath(temp_f.path().to_path_buf()));
    }
}
