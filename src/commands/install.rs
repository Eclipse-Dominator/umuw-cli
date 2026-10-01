use anyhow::{Ok, Result};

use crate::{
    commands::run::{LaunchCtx, run},
    config::{self, Group, SaveableToml},
    dmenu,
};

#[derive(Debug, PartialEq)]
pub enum InstallResult<'a> {
    GroupAlrExist(&'a str),
    GroupNotFound(&'a str),
    InstallRun(i32),
}

pub fn install<'a, D: dmenu::DMenuAdapter>(
    ctx: LaunchCtx<'_, D>,
    installer: &str,
    group: Option<(&'a str, bool)>,
    args: Vec<String>,
) -> Result<InstallResult<'a>> {
    let code = match group {
        Some((name, new)) => {
            match Group::ensure(ctx.config_root, name, new)? {
                config::EnsureOutcome::AlrExist(s) => return Ok(InstallResult::GroupAlrExist(s)),
                config::EnsureOutcome::NotFound(s) => return Ok(InstallResult::GroupNotFound(s)),
                config::EnsureOutcome::Ensured(_) => {}
            };
            run(&ctx, installer, Some(name), args)?
        }

        None => run(&ctx, installer, None, args)?,
    };
    Ok(InstallResult::InstallRun(code))
}

#[cfg(test)]
mod tests {
    use tempfile::{NamedTempFile, TempDir};

    use super::*;
    use crate::commands::run::tests_util::TestCtx;
    use crate::dmenu::tests_util::FirstChoiceAdapter;

    #[test]
    fn install_creates_seeded_group_when_missing() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        let temp_f = TempDir::new().unwrap();
        let config_root = ctx.config_root;
        let res = install(
            ctx,
            &temp_f.path().to_string_lossy(),
            Some(("unknown group", true)),
            vec![],
        );
        assert!(res.is_err());
        assert!(Group::toml_path(config_root, "unknown group").exists());
    }

    #[test]
    fn install_refuses_when_group_exists() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        let f = NamedTempFile::new().unwrap();
        Group::default().save(ctx.config_root, "somegrp").unwrap();
        let res = install(
            ctx,
            &f.path().to_string_lossy(),
            Some(("somegrp", true)),
            vec![],
        )
        .unwrap();
        assert_eq!(res, InstallResult::GroupAlrExist("somegrp"));
    }

    #[test]
    fn install_refuses_when_group_not_found() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        let f = NamedTempFile::new().unwrap();
        let res = install(
            ctx,
            &f.path().to_string_lossy(),
            Some(("somegrp", false)),
            vec![],
        )
        .unwrap();
        assert_eq!(res, InstallResult::GroupNotFound("somegrp"));
    }
}
