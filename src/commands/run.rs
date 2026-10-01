use std::{env, ffi::OsString, path::Path, process, vec};

use anyhow::{Context, Result, bail};

use crate::{
    config::{Effective, FallbackPath, Game, Global, Group, SaveableToml, resolve_effective},
    dmenu,
    gamescope::{self, GamescopeSpec},
    paths::{self, obtain_proton_path},
    prefix::{self, EnsureDrivesRes},
};

const USER_CANCELLED_MSG: &str = "User Cancelled!";

pub struct LaunchCtx<'a, D: dmenu::DMenuAdapter> {
    pub dmenu: &'a D,
    pub config_root: &'a Path,
    pub home: &'a Path,
    pub local_app_root: &'a Path,
    pub icon_root: &'a Path,
    pub proton_search_folders: &'a [paths::ProtonPathOrigin],
    pub gs_passthrough_env: &'a [(&'a str, Option<OsString>)],
    pub editor: Option<&'a str>,
}

pub fn run<D: dmenu::DMenuAdapter>(
    ctx: &LaunchCtx<D>,
    item: &str,
    group: Option<&str>,
    args: Vec<String>,
) -> Result<i32> {
    let eff = get_effective(ctx.config_root, item, group, args)?;
    let gamescope_prompt = gamescope::obtain_gamescope_spec(
        ctx.dmenu,
        &eff.gamescope_opts,
        eff.gamescope_mode,
        &gamescope::create_fallback_gamescope_spec(&eff),
    )?;

    let gamescope_spec = match gamescope_prompt {
        gamescope::PromptOutcome::Cancelled => return Ok(0),
        gamescope::PromptOutcome::Native => None,
        gamescope::PromptOutcome::Gamescope(spec) => Some(spec),
    };

    let mut cmd = get_effective_command(
        ctx.home,
        &eff,
        ctx.proton_search_folders,
        gamescope_spec,
        ctx.gs_passthrough_env,
    )?;

    let prefix_path = prefix::prefix_path(&eff.prefix_fragment, ctx.home);
    match prefix::ensure_drives(&prefix_path, &eff.drives)? {
        EnsureDrivesRes::Complete => {}
        EnsureDrivesRes::CompleteWithFailure => {
            eprintln!("warning: some drives cant be linked, the launch might be abnormal")
        }
        EnsureDrivesRes::InvalidPrefix(s) => eprintln!(
            "warning: prefix {} seems to be missing, this launch might be abnormal.
             prefix will be created this launch; drives link on the next run",
            s.display()
        ),
    }
    run_effective(&mut cmd)
}

pub fn dry_run<D: dmenu::DMenuAdapter>(
    ctx: &LaunchCtx<D>,
    item: &str,
    group: Option<&str>,
    args: Vec<String>,
) -> Result<String> {
    let eff = get_effective(ctx.config_root, item, group, args)?;
    let gamescope_prompt = gamescope::obtain_gamescope_spec(
        ctx.dmenu,
        &eff.gamescope_opts,
        eff.gamescope_mode,
        &gamescope::create_fallback_gamescope_spec(&eff),
    )?;

    let gamescope_spec = match gamescope_prompt {
        gamescope::PromptOutcome::Cancelled => return Ok(USER_CANCELLED_MSG.to_owned()),
        gamescope::PromptOutcome::Native => None,
        gamescope::PromptOutcome::Gamescope(spec) => Some(spec),
    };
    let cmd = get_effective_command(
        ctx.home,
        &eff,
        ctx.proton_search_folders,
        gamescope_spec,
        ctx.gs_passthrough_env,
    )?;
    Ok(command_to_string(&cmd))
}

pub fn get_effective(
    config_root: &Path,
    item: &str,
    group: Option<&str>,
    args: Vec<String>,
) -> Result<Effective> {
    let (game, name) = if Game::toml_path(config_root, item).exists() {
        let mut game =
            Game::load(config_root, item).with_context(|| format!("Finding game {}", item))?;
        if !args.is_empty() {
            game.args = args;
        }
        (game, Some(item))
    } else if Path::new(item).exists() {
        (
            Game {
                app_entry: Some(item.to_owned()),
                args,
                ..Default::default()
            },
            None,
        )
    } else {
        bail!("{item} is not a valid game or path!");
    };
    let global = Global::get_user_default(config_root)?;
    let group = group.or(game.group.as_deref());
    let (grp_name, grp) = match group {
        Some(grp_name) => {
            let grp = Group::load(config_root, grp_name)?;
            (grp_name, grp)
        }
        None => (Group::DEFAULT_CONFIG, Group::get_user_default(config_root)?),
    };
    resolve_effective(grp_name, name, &global, &grp, &game)
}

fn command_to_string(cmd: &process::Command) -> String {
    let mut env_set: Vec<String> = vec![];
    let mut env_removed: Vec<String> = vec![];
    for (k, v) in cmd.get_envs() {
        match v {
            Some(v) => {
                let v = v.to_string_lossy().to_string();
                if v.contains(" ") {
                    env_set.push(format!("{}=\"{}\"", k.to_string_lossy(), v));
                } else {
                    env_set.push(format!("{}={}", k.to_string_lossy(), v));
                }
            }
            None => env_removed.push(format!("{} (removed)", k.to_string_lossy())),
        };
    }

    let args: Vec<String> = cmd
        .get_args()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
    let prog = cmd.get_program().to_string_lossy().to_string();

    let mut ret = String::new();
    ret += &env_removed.join("\n");
    ret += "\n";
    ret += &env_set.join(" ");
    ret += " ";
    ret += &prog;
    ret += " ";
    ret += &args.join(" ");
    ret
}

fn build_env(
    home: &Path,
    eff: &Effective,
    proton_search_folders: &[paths::ProtonPathOrigin],
) -> Result<Vec<(String, OsString)>> {
    if env::var_os("WINEPREFIX").is_some_and(|s| !s.is_empty()) {
        eprintln!("warning: umuw manages the prefix; WINEPREFIX is ignored");
    }

    let mut ret = vec![];
    let proton_path = obtain_proton_path(&eff.runner, proton_search_folders)
        .with_context(|| format!("obtaining proton: {}", &eff.runner))?;

    if let Some(store) = &eff.store {
        ret.push(("STORE".to_owned(), OsString::from(store)));
    }
    ret.push(("GAMEID".to_owned(), OsString::from(&eff.gameid)));
    ret.push(("PROTONPATH".to_owned(), proton_path.into_os_string()));

    let prefix_path = prefix::prefix_path(&eff.prefix_fragment, home);
    ret.push(("WINEPREFIX".to_owned(), prefix_path.into()));

    for eff_env in &eff.env {
        match eff_env.split_once("=") {
            None => eprintln!("warning: ignoring env: {}", eff_env),
            Some((key, val)) => ret.push((key.to_owned(), OsString::from(val))),
        };
    }

    Ok(ret)
}

fn create_cmd(
    eff: &Effective,
    gamescope_spec: Option<GamescopeSpec>,
    gs_passthrough_env: &[(&str, Option<OsString>)],
) -> std::process::Command {
    // returns a Command that ends with umu-run
    match gamescope_spec {
        Some(spec) => {
            let mut c = gamescope::create_gamescope_cmd(&spec, gs_passthrough_env);
            if let Some(wrapper) = eff.wrapper.as_deref() {
                c.arg(shellexpand::tilde(wrapper).as_ref());
            }
            c.arg("umu-run");
            c
        }
        None => match eff.wrapper.as_deref() {
            Some(wrapper) => {
                let mut c = std::process::Command::new(shellexpand::tilde(wrapper).to_string());
                c.arg("umu-run");
                c
            }
            None => std::process::Command::new("umu-run"),
        },
    }
}

fn get_effective_command(
    home: &Path,
    eff: &Effective,
    proton_search_folders: &[paths::ProtonPathOrigin],
    gamescope_spec: Option<GamescopeSpec>,
    gs_passthrough_env: &[(&str, Option<OsString>)],
) -> Result<process::Command> {
    let env_vars = build_env(home, eff, proton_search_folders)?;
    let mut cmd = create_cmd(eff, gamescope_spec, gs_passthrough_env);

    if eff.store.is_none() {
        cmd.env_remove("STORE");
    };

    cmd.arg(
        eff.app_entry
            .as_deref()
            .context("No entry point provided")?,
    )
    .envs(env_vars);

    if !eff.args.is_empty() {
        cmd.args(&eff.args);
    }

    Ok(cmd)
}

fn run_effective(cmd: &mut process::Command) -> Result<i32> {
    let exit_status = cmd.status().with_context(|| "running umu-run")?;

    Ok(exit_status.code().unwrap_or(130))
}

#[cfg(test)]
pub mod tests_util {
    use std::{collections::HashMap, fs, path::PathBuf};

    use tempfile::TempDir;

    use crate::{config::Seeded, paths::ProtonPathOrigin};

    use super::*;
    pub struct TestCtx<'a> {
        pub config_root: TempDir,
        pub home: TempDir,
        pub local_app_dir: TempDir,
        pub icon_dir: TempDir,
        pub _proton_tmp_folders: Vec<TempDir>,
        pub proton_search_folders: Vec<ProtonPathOrigin>,
        pub proton_map: HashMap<&'a str, PathBuf>,
    }

    impl<'a> TestCtx<'a> {
        pub fn new(proton_names: &[&'a str]) -> Self {
            let local_app_dir = TempDir::new().context("creating local app dir").unwrap();
            let icon_dir = TempDir::new().context("creating icon dir").unwrap();
            let home_dir = TempDir::new().context("creating home dir").unwrap();
            let config_dir = TempDir::new().context("creating tmp config dir").unwrap();
            let mut _proton_tmp_folders = vec![];
            let mut proton_map = HashMap::<&'a str, PathBuf>::new();
            for proton_name in proton_names {
                let proton_dir = TempDir::new().context("creating tmp proton dir").unwrap();
                let proton_location = proton_dir.path().join(proton_name);
                fs::create_dir(&proton_location).unwrap();
                _proton_tmp_folders.push(proton_dir);
                proton_map.insert(*proton_name, proton_location);
            }
            Group::seed().save(config_dir.path(), "default").unwrap();
            let proton_search_folders: Vec<ProtonPathOrigin> = _proton_tmp_folders
                .iter()
                .map(|s| ProtonPathOrigin::Absolute(s.path().to_string_lossy().to_string()))
                .collect();

            TestCtx {
                home: home_dir,
                config_root: config_dir,
                _proton_tmp_folders,
                local_app_dir,
                icon_dir,
                proton_search_folders,
                proton_map,
            }
        }

        pub fn ctx<D: dmenu::DMenuAdapter>(
            &'a self,
            dmenu: &'a D,
            gs_passthrough_env: &'a [(&str, Option<OsString>)],
        ) -> LaunchCtx<'a, D> {
            LaunchCtx {
                dmenu,
                config_root: self.config_root.path(),
                local_app_root: self.local_app_dir.path(),
                icon_root: self.icon_dir.path(),
                home: self.home.path(),
                proton_search_folders: &self.proton_search_folders,
                gs_passthrough_env,
                editor: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        commands::{
            new::{self, ConfigStatus},
            run::tests_util::TestCtx,
        },
        config::{GamescopeMode, Seeded},
        dmenu::tests_util::*,
    };

    use super::*;
    use tempfile::{NamedTempFile, TempDir};

    #[test]
    fn get_effective_correctly_matches_get_effective() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_dir.path();
        let init_group = Group::seed();
        let grp = Group {
            runner: Some("proton-cachyos-native".to_owned()),
            ..Default::default()
        };
        let game = Game {
            group: Some("test_group".to_owned()),
            app_entry: Some("dummy".to_owned()),
            ..Default::default()
        };
        init_group.save(root, "default").unwrap();
        grp.save(root, "test_group").unwrap();
        game.save(root, "sample_game").unwrap();

        let eff = get_effective(root, "sample_game", None, vec![]).unwrap();
        let expected = resolve_effective(
            "test_group",
            Some("sample_game"),
            &Global::default(),
            &grp,
            &game,
        )
        .unwrap();
        assert_eq!(eff, expected);
    }

    #[test]
    fn get_effective_correctly_uses_specified_group() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_dir.path();
        let init_group = Group::seed();
        let grp = Group {
            runner: Some("proton-cachyos-native".to_owned()),
            ..Default::default()
        };

        let grp2 = Group {
            runner: Some("group2-proton".to_owned()),
            ..Default::default()
        };

        let game = Game {
            group: Some("test_group".to_owned()),
            app_entry: Some("dummy".to_owned()),
            ..Default::default()
        };
        init_group.save(root, "default").unwrap();
        grp.save(root, "test_group").unwrap();
        grp2.save(root, "test_group_prefered").unwrap();
        game.save(root, "sample_game").unwrap();

        let eff = get_effective(root, "sample_game", Some("test_group_prefered"), vec![]).unwrap();
        let expected = resolve_effective(
            "test_group_prefered",
            Some("sample_game"),
            &Global::default(),
            &grp2,
            &game,
        )
        .unwrap();
        assert_eq!(eff, expected);
    }

    #[test]
    fn build_env_correctly_account_for_env() {
        let proton_name = "some_runner";
        let mock = TestCtx::new(&[proton_name]);
        let proton_location = mock.proton_map.get(proton_name).unwrap();

        let eff = Effective {
            app_entry: Some("entry pt".to_string()),
            env: vec!["test=1234".to_owned(), "harro=123".to_owned()],
            gameid: "somegameid".to_string(),
            runner: "some_runner".to_string(),
            prefix_fragment: "some/path/test test".to_string(),
            ..Default::default()
        };

        let mut expected = vec![
            ("GAMEID".to_owned(), OsString::from("somegameid")),
            ("PROTONPATH".to_owned(), proton_location.into()),
            ("test".to_string(), OsString::from("1234")),
            ("harro".to_string(), OsString::from("123")),
            (
                "WINEPREFIX".to_string(),
                prefix::prefix_path(&eff.prefix_fragment, mock.home.path()).into(),
            ),
        ];
        expected.sort();

        let mut res = build_env(mock.home.path(), &eff, &mock.proton_search_folders).unwrap();
        res.sort();

        assert_eq!(res, expected);
    }

    #[test]
    fn build_env_correctly_accounts_for_store() {
        let proton_name = "some_runner";
        let mock = TestCtx::new(&[proton_name]);
        let proton_loc = mock.proton_map.get(proton_name).unwrap();

        let eff = Effective {
            app_entry: Some("entry pt".to_string()),
            gameid: "somegameid".to_string(),
            runner: "some_runner".to_string(),
            store: Some("some_store".to_string()),
            prefix_fragment: "test/asdf/asdf".to_string(),
            ..Default::default()
        };

        let mut expected = vec![
            ("STORE".to_owned(), OsString::from("some_store")),
            ("GAMEID".to_owned(), OsString::from("somegameid")),
            ("PROTONPATH".to_owned(), proton_loc.into()),
            (
                "WINEPREFIX".to_owned(),
                prefix::prefix_path(&eff.prefix_fragment, mock.home.path()).into(),
            ),
        ];
        expected.sort();

        let mut res = build_env(mock.home.path(), &eff, &mock.proton_search_folders).unwrap();
        res.sort();

        assert_eq!(res, expected);
    }

    #[test]
    fn get_effective_command_panics_on_missing_entry_pt() {
        let proton_name = "example-proton latest";
        let mock = TestCtx::new(&[proton_name]);
        let grp = Group {
            store: Some("gog".to_owned()),
            runner: Some(proton_name.to_owned()),
            ..Default::default()
        };
        let game = Game {
            group: Some("test_group".to_owned()),
            app_entry: None,
            gamescope_mode: Some(GamescopeMode::Native),
            ..Default::default()
        };
        grp.save(mock.config_root.path(), "test_group").unwrap();
        game.save(mock.config_root.path(), "sample_game").unwrap();
        let eff = get_effective(
            mock.config_root.path(),
            "sample_game",
            Some("test_group"),
            vec![],
        )
        .unwrap();
        let res = get_effective_command(
            mock.home.path(),
            &eff,
            &mock.proton_search_folders,
            None,
            &[],
        );
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("No entry point provided")
        )
    }

    #[test]
    fn dry_run_support_space() {
        let proton_name = "example-proton latest";
        let mock = TestCtx::new(&[proton_name]);
        let grp = Group {
            runner: Some(proton_name.to_owned()),
            ..Default::default()
        };
        let game = Game {
            group: Some("test_group".to_owned()),
            app_entry: Some("/some-dummy".to_owned()),
            gamescope_mode: Some(GamescopeMode::Native),
            ..Default::default()
        };
        grp.save(mock.config_root.path(), "test_group").unwrap();
        game.save(mock.config_root.path(), "sample game").unwrap();

        let res = dry_run(&mock.ctx(&TimeoutAdapter, &[]), "sample game", None, vec![]).unwrap();
        assert!(res.contains("STORE (removed)"));
        assert!(res.contains("umu-run /some-dummy"));
        assert!(res.contains(&format!(
            "PROTONPATH=\"{}\"",
            mock.proton_map.get(proton_name).unwrap().to_string_lossy()
        )));
        assert!(res.contains("GAMEID=umu-sample-game"));
    }

    #[test]
    fn dry_run_matches_correct_string() {
        let proton_name = "example-proton latest";
        let mock = TestCtx::new(&[proton_name]);
        let grp = Group {
            runner: Some(proton_name.to_owned()),
            ..Default::default()
        };
        let game = Game {
            group: Some("test_group".to_owned()),
            app_entry: Some("/some-dummy".to_owned()),
            gamescope_mode: Some(GamescopeMode::Native),
            ..Default::default()
        };
        grp.save(mock.config_root.path(), "test_group").unwrap();
        game.save(mock.config_root.path(), "sample_game").unwrap();

        let res = dry_run(&mock.ctx(&TimeoutAdapter, &[]), "sample_game", None, vec![]).unwrap();
        assert!(res.contains("STORE (removed)"));
        assert!(res.contains("umu-run /some-dummy"));
        assert!(res.contains(&format!(
            "PROTONPATH=\"{}\"",
            mock.proton_map.get(proton_name).unwrap().to_string_lossy()
        )));
        assert!(res.contains("GAMEID=umu-sample_game"));
    }

    #[test]
    fn dry_run_supports_different_gameid_sharing_same_group() {
        let proton_name = "example-proton latest";
        let mock = TestCtx::new(&[proton_name]);
        let grp = Group {
            store: Some("gog".to_owned()),
            runner: Some(proton_name.to_owned()),
            ..Default::default()
        };
        let game = Game {
            group: Some("test_group".to_owned()),
            gameid: Some("game1_id".to_owned()),
            app_entry: Some("/some-dummy".to_owned()),
            gamescope_mode: Some(GamescopeMode::Native),
            ..Default::default()
        };
        let game2 = Game {
            group: Some("test_group".to_owned()),
            app_entry: Some("/some-dummy".to_owned()),
            gamescope_mode: Some(GamescopeMode::Native),
            ..Default::default()
        };
        grp.save(mock.config_root.path(), "test_group").unwrap();
        game.save(mock.config_root.path(), "sample_game").unwrap();
        game2.save(mock.config_root.path(), "sample_game2").unwrap();

        let dmenu = TimeoutAdapter;
        let ctx = mock.ctx(&dmenu, &[]);

        let res = dry_run(&ctx, "sample_game", None, vec![]).unwrap();
        let res2 = dry_run(&ctx, "sample_game2", None, vec![]).unwrap();
        assert!(res.contains("GAMEID=game1_id"));
        assert!(res2.contains("GAMEID=umu-sample_game2"));
    }

    #[test]
    fn dry_run_returns_cancelled_msg_when_cancelled() {
        let proton_name = "example-proton latest";
        let mock = TestCtx::new(&[proton_name]);
        let grp = Group::default();
        let game = Game {
            group: Some("test_group".to_owned()),
            app_entry: Some("/some-dummy".to_owned()),
            gamescope_mode: Some(GamescopeMode::Prompt),
            ..Default::default()
        };
        grp.save(mock.config_root.path(), "test_group").unwrap();
        game.save(mock.config_root.path(), "sample_game").unwrap();

        let res = dry_run(&mock.ctx(&CancelAdapter, &[]), "sample_game", None, vec![]).unwrap();
        assert!(res.contains(USER_CANCELLED_MSG));
    }

    #[test]
    fn create_cmd_supports_wrapper_with_relative_path_in_gamescope() {
        let eff = Effective {
            wrapper: Some("~/relative/path".to_owned()),
            ..Default::default()
        };
        let wrapper_full_path = env::home_dir()
            .unwrap()
            .join("relative/path")
            .to_string_lossy()
            .to_string();

        let spec = GamescopeSpec::default();
        let cmd = create_cmd(&eff, Some(spec), &[]);
        assert!(command_to_string(&cmd).contains(&format!(
            "-- env ENABLE_GAMESCOPE_WSI=1 {} umu-run",
            wrapper_full_path
        )))
    }

    #[test]
    fn create_cmd_supports_wrapper_with_relative_path_in_native() {
        let eff = Effective {
            wrapper: Some("~/relative/path".to_owned()),
            ..Default::default()
        };
        let home = env::home_dir()
            .unwrap()
            .join("relative/path")
            .to_string_lossy()
            .to_string();
        let cmd = create_cmd(&eff, None, &[]);
        assert!(cmd.get_program().to_string_lossy().contains(&home));
    }

    #[test]
    fn config_written_profiles_resolve_through_get_effective() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&FirstChoiceAdapter, &[]);
        let exe = NamedTempFile::new().unwrap();
        let config_status = new::config(
            ctx.config_root,
            exe.path().to_string_lossy().to_string(),
            Some(("testgrp", true)),
            Some("Testgame".to_owned()),
        )
        .unwrap();
        assert_eq!(config_status, ConfigStatus::Success("Testgame".to_owned()));
        let eff = get_effective(ctx.config_root, "Testgame", None, vec![]).unwrap();
        assert_eq!(eff.app_entry.as_deref(), exe.path().to_str());
        assert_eq!(eff.prefix_fragment, "testgrp");
        assert_eq!(eff.gameid, "umu-testgame");
    }

    #[test]
    fn dry_run_args_replaces_profile_args() {
        let t_ctx = TestCtx::new(&["dummy"]);
        let ctx = t_ctx.ctx(&CancelAdapter, &[]);
        let game = Game {
            group: Some("test_group".to_owned()),
            app_entry: Some("/some-dummy".to_owned()),
            gamescope_mode: Some(GamescopeMode::Native),
            args: vec!["--arg1".into(), "val1".into(), "--arg2".into()],
            ..Default::default()
        };
        game.save(ctx.config_root, "sample_game").unwrap();
        Group {
            runner: Some("dummy".into()),
            ..Group::seed()
        }
        .save(ctx.config_root, "test_group")
        .unwrap();

        let res = dry_run(&ctx, "sample_game", None, vec![]).unwrap();
        assert!(res.contains("--arg1 val1 --arg2"));

        let res = dry_run(
            &ctx,
            "sample_game",
            None,
            vec!["--windowed".into(), "--fps".into()],
        )
        .unwrap();
        assert!(res.contains("--windowed --fps"));
    }

    #[test]
    fn dry_run_supports_path_based_runs_and_args() {
        let t = TestCtx::new(&["proton"]);
        let ctx = t.ctx(&FirstChoiceAdapter, &[]);
        let exe = NamedTempFile::new().unwrap();

        let mut g = Group::get_user_default(ctx.config_root).unwrap();
        g.runner = Some("proton".into());
        g.save(ctx.config_root, Group::DEFAULT_CONFIG).unwrap();
        let exe_str = exe.path().to_string_lossy();

        let res = dry_run(&ctx, &exe_str, None, vec!["--flag".into(), "value".into()]).unwrap();

        assert!(res.contains(&format!("umu-run {exe_str} --flag value")));
        assert!(res.contains("GAMEID=umu-default")); // ad-hoc identity unchanged
    }
}
