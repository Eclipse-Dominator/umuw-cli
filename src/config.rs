use std::{
    any::type_name,
    collections::BTreeMap,
    fs::{self},
    path::{Path, PathBuf},
    vec,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    dmenu, paths,
    prefix::{DriveMapping, parse_drive_mapping},
};

const DEFAULT_RUNNER: &str = "proton-cachyos-native";

pub trait Seeded: Default {
    // creates default seeded struct (fallback to default)
    fn seed() -> Self {
        Self::default()
    }
}

pub trait SaveableToml: Serialize + serde::de::DeserializeOwned + Seeded {
    const SUBDIR: &'static str;

    fn dir(config_root: &Path) -> PathBuf {
        config_root.join(Self::SUBDIR)
    }

    fn toml_path(config_root: &Path, name: &str) -> PathBuf {
        Self::dir(config_root).join(format!("{}.toml", name))
    }

    fn ensure_config_folder(config_root: &Path) -> Result<PathBuf> {
        let config_file_folder = Self::dir(config_root);
        if !config_file_folder.exists() {
            fs::create_dir(&config_file_folder)
                .with_context(|| format!("Creating {}", config_file_folder.display()))?;
        }
        Ok(config_file_folder)
    }

    fn ensure<'a>(config_root: &Path, name: &'a str, new: bool) -> Result<EnsureOutcome<'a>> {
        let path = Self::toml_path(config_root, name);
        match (path.exists(), new) {
            (false, false) => return Ok(EnsureOutcome::NotFound(name)),
            (true, true) => return Ok(EnsureOutcome::AlrExist(name)),
            (false, true) => Self::seed().save(config_root, name)?,
            (_, _) => {}
        };
        Ok(EnsureOutcome::Ensured(name))
    }

    fn save(&self, config_root: &Path, name: &str) -> Result<()> {
        // creates the folder if non existant then write to the relevant file
        let config_file_path =
            Self::ensure_config_folder(config_root)?.join(format!("{}.toml", name));
        let to_write = toml::to_string_pretty(self)
            .with_context(|| format!("serializing struct to toml ({})", name))?;
        fs::write(&config_file_path, to_write)
            .with_context(|| format!("writing {}", config_file_path.display()))
    }

    fn load(config_root: &Path, name: &str) -> Result<Self> {
        load_profile::<Self>(&Self::toml_path(config_root, name))
    }

    fn edit(config_root: &Path, name: &str, editor: Option<&str>) -> Result<Option<i32>> {
        let file_path = Self::toml_path(config_root, name);
        paths::edit(
            file_path
                .to_str()
                .with_context(|| format!("obtaining path for {name}"))?,
            editor,
        )
    }

    fn rename<'a>(config_root: &Path, name_from: &'a str, name_to: &'a str) -> Result<&'a str> {
        rename_toml::<Self>(config_root, name_from, name_to)
    }
}

pub trait FallbackPath: SaveableToml {
    // denote trait where the struct as a default fallback file and return its path
    const DEFAULT_CONFIG: &'static str;

    fn default_path(config_root: &Path) -> PathBuf {
        Self::toml_path(config_root, Self::DEFAULT_CONFIG)
    }

    fn get_user_default(config_root: &Path) -> Result<Self> {
        if Self::default_path(config_root).exists() {
            return Self::load(config_root, Self::DEFAULT_CONFIG);
        }
        Ok(Self::seed())
    }
}

#[derive(Debug, PartialEq)]
pub enum EnsureOutcome<'a> {
    AlrExist(&'a str),
    NotFound(&'a str),
    Ensured(&'a str),
}

#[derive(Default, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GamescopeMode {
    #[default]
    Prompt,
    Gamescope,
    Native,
}

#[derive(Default, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DmenuPreference {
    #[default]
    Noctalia,
    Rofi,
}

impl dmenu::DMenuAdapter for DmenuPreference {
    fn choose(&self, prompt: &str, options: &[&str], timeout_secs: u64) -> Result<dmenu::Pick> {
        match &self {
            DmenuPreference::Noctalia => {
                dmenu::NoctaliaAdapter.choose(prompt, options, timeout_secs)
            }
            DmenuPreference::Rofi => dmenu::RofiAdapter.choose(prompt, options, timeout_secs),
        }
    }
}

#[derive(Default, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct UmuwConfig {
    /// settings group
    pub setting: Option<Setting>,
    /// global group for global settings to fallback on
    pub global: Option<Global>,
}

impl Seeded for UmuwConfig {}

#[derive(Default, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Setting {
    /// dmenu launcher that will be used to prompt user, defaults to Noctalia
    pub dmenu: DmenuPreference,

    /// preferred editor
    pub editor: Option<String>,
}

impl Seeded for Setting {}

#[derive(Default, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Global {
    /// The preferred proton to be used to run application if not set
    pub runner: Option<String>,
    /// default list of gamescope launch options
    pub gamescope_opts: Option<Vec<String>>,
    /// default way to launch applicaiton: prompt | gamescope | native.
    /// gamescope will use the first option from gamescope_opts
    pub gamescope_mode: Option<GamescopeMode>,
    /// chooses if hdr is enabled by default
    pub hdr: Option<bool>,
    /// additional wrapper to wrap around the default launch commands
    pub wrapper: Option<String>,
    /// keywords to be included in .desktop
    pub keywords: Option<Vec<String>>,
}

impl Seeded for Global {}

#[derive(Default, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Group {
    /// The preferred proton to be used to run application if not set
    pub runner: Option<String>,
    /// default list of env variables to be used in this group
    #[serde(default)]
    pub env: Vec<String>,
    /// default list of drives to be mounted in this group
    #[serde(default)]
    pub drives: Vec<String>,
    /// default list of gamescope launch options
    pub gamescope_opts: Option<Vec<String>>,
    /// default way to launch applicaiton: prompt | gamescope | native.
    /// gamescope will use the first option from gamescope_opts
    pub gamescope_mode: Option<GamescopeMode>,
    /// chooses if hdr is enabled by default
    pub hdr: Option<bool>,
    /// choose if the games/application belong in this group belong to certain stores
    /// used with gameid to lookup umu db
    pub store: Option<String>,
    /// additional wrapper to wrap around the default launch commands
    pub wrapper: Option<String>,
    /// keywords to be included in .desktop
    #[serde(default)]
    pub keywords: Vec<String>,
}

pub fn resolved_gameid(gameid: Option<&str>, name: &str) -> String {
    gameid.map(str::to_owned).unwrap_or_else(|| {
        let mut name = name.replace(" ", "-");
        name.make_ascii_lowercase();
        format!("umu-{name}")
    })
}

impl Seeded for Group {
    fn seed() -> Self {
        Group {
            runner: Some(DEFAULT_RUNNER.to_owned()),
            gamescope_mode: Some(GamescopeMode::Prompt),
            gamescope_opts: Some(default_gamescope_option()),
            ..Default::default()
        }
    }
}

#[derive(Default, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Game {
    /// Group that the game belong to, if left empty, it will use the "default" group
    pub group: Option<String>,
    /// store to be used with gameid to download umu fixes from umu db. default to ""
    pub store: Option<String>,
    /// gameid to be used with store to download umu fixes from umu db. default to umu-default
    pub gameid: Option<String>, // fixes identity ONLY — protonfixes key; prefix path unaffected
    /// wineprefix override if not using the default wine prefix
    pub prefix: Option<String>, // isolated prefix if not sharing with group's prefix
    /// entry to the application (.exe, .msi...)
    pub app_entry: Option<String>,
    /// Launch options using gamescope for this application
    pub gamescope_opts: Option<Vec<String>>,
    /// default way to launch applicaiton: prompt | gamescope | native.
    /// gamescope will use the first option from gamescope_opts
    pub gamescope_mode: Option<GamescopeMode>,
    /// whether to run in hdr or not
    pub hdr: Option<bool>,
    /// additional environment variables to be included with launching this application
    #[serde(default)]
    pub env: Vec<String>,
    /// additional wrapper to wrap around the default launch commands
    pub wrapper: Option<String>,
    /// keywords to be included in .desktop
    #[serde(default)]
    pub keywords: Vec<String>,
    /// flags to be added with the entry
    #[serde(default)]
    pub args: Vec<String>,
}

impl Seeded for Game {}

impl SaveableToml for UmuwConfig {
    const SUBDIR: &'static str = "";
}

impl SaveableToml for Setting {
    const SUBDIR: &'static str = "";

    fn load(config_root: &Path, name: &str) -> Result<Self> {
        // fallback to load using UmuwConfig
        let res = UmuwConfig::load(config_root, name)?;
        Ok(res.setting.unwrap_or_default())
    }

    fn save(&self, config_root: &Path, name: &str) -> Result<()> {
        let toml_path = UmuwConfig::toml_path(config_root, name);
        let mut umuw_config = if toml_path.exists() {
            UmuwConfig::load(config_root, name)?
        } else {
            UmuwConfig::default()
        };
        umuw_config.setting = Some(self.clone());
        umuw_config.save(config_root, name)?;
        Ok(())
    }
}

impl SaveableToml for Global {
    const SUBDIR: &'static str = "";

    fn load(config_root: &Path, name: &str) -> Result<Self> {
        // fallback to load using UmuwConfig
        let res = UmuwConfig::load(config_root, name)?;
        Ok(res.global.unwrap_or_default())
    }

    fn save(&self, config_root: &Path, name: &str) -> Result<()> {
        let toml_path = UmuwConfig::toml_path(config_root, name);
        let mut umuw_config = if toml_path.exists() {
            UmuwConfig::load(config_root, name)?
        } else {
            UmuwConfig::default()
        };
        umuw_config.global = Some(self.clone());
        umuw_config.save(config_root, name)?;
        Ok(())
    }

    fn edit(config_root: &Path, name: &str, editor: Option<&str>) -> Result<Option<i32>> {
        let toml_path = UmuwConfig::toml_path(config_root, name);
        if !toml_path.exists() {
            let config = UmuwConfig::get_user_default(config_root)?;
            config.save(config_root, name)?;
        }
        UmuwConfig::edit(config_root, name, editor)
    }
}

impl SaveableToml for Group {
    const SUBDIR: &'static str = "groups";

    fn rename<'a>(config_root: &Path, name_from: &'a str, name_to: &'a str) -> Result<&'a str> {
        if name_from == Group::DEFAULT_CONFIG {
            bail!("default group cannot be renamed!");
        }

        rename_toml::<Group>(config_root, name_from, name_to)
    }
}

impl SaveableToml for Game {
    const SUBDIR: &'static str = "games";
}

impl FallbackPath for UmuwConfig {
    const DEFAULT_CONFIG: &'static str = "umuw";
}

impl FallbackPath for Global {
    const DEFAULT_CONFIG: &'static str = "umuw";

    fn default_path(_: &Path) -> PathBuf {
        panic!("Global is part of UmuwConfig, this value should NEVER be used")
    }

    fn get_user_default(config_root: &Path) -> Result<Self> {
        let config = UmuwConfig::get_user_default(config_root)?;
        Ok(config.global.unwrap_or_default())
    }
}

impl FallbackPath for Setting {
    const DEFAULT_CONFIG: &'static str = "umuw";

    fn default_path(_: &Path) -> PathBuf {
        panic!("Setting is part of UmuwConfig, this value should NEVER be used")
    }

    fn get_user_default(config_root: &Path) -> Result<Self> {
        let config = UmuwConfig::get_user_default(config_root)?;
        Ok(config.setting.unwrap_or_default())
    }
}

impl FallbackPath for Group {
    const DEFAULT_CONFIG: &'static str = "default";
}

// merged effective launch option
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Effective {
    pub runner: String,
    pub store: Option<String>,
    pub gameid: String,
    pub prefix_fragment: String,
    pub gamescope_opts: Vec<String>,
    pub gamescope_mode: GamescopeMode,
    pub hdr: Option<bool>,
    pub env: Vec<String>,
    pub app_entry: Option<String>,
    pub wrapper: Option<String>,
    pub drives: Vec<DriveMapping>,
    pub keywords: Vec<String>,
    pub args: Vec<String>,
}

pub fn load_profile<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
    let result = toml::from_str::<T>(&content).with_context(|| {
        format!(
            "Parsing file {} to type {}",
            path.display(),
            type_name::<T>()
        )
    })?;
    Ok(result)
}
pub fn list_all_profile_toml<T: SaveableToml>(config_root: &Path) -> Result<Vec<PathBuf>> {
    // returns path to the said profile given the config root
    let config_dir = T::dir(config_root);
    let mut ret = vec![];
    let files =
        fs::read_dir(&config_dir).with_context(|| format!("accessing {}", config_dir.display()))?;
    for file in files {
        let f = file?.path();
        if f.is_file()
            && f.extension()
                .and_then(|ext| ext.to_str().map(|ext_str| ext_str == "toml"))
                .unwrap_or(false)
        {
            ret.push(f);
        }
    }
    Ok(ret)
}

pub fn load_all<T: SaveableToml>(config_root: &Path) -> Result<BTreeMap<String, T>> {
    let mut ret = BTreeMap::<String, T>::new();
    for path in list_all_profile_toml::<T>(config_root)? {
        match path.file_stem().and_then(|stem| stem.to_str()) {
            Some(name) => match load_profile::<T>(&path) {
                Ok(profile) => {
                    ret.insert(name.to_owned(), profile);
                }

                Err(e) => eprintln!("warning: skipping {}: {e:#}", path.display()),
            },
            None => eprintln!(
                "warning: unable to parse stem of {}, skipping",
                path.display()
            ),
        }
    }

    Ok(ret)
}

pub fn resolve_effective(
    grp_name: &str,
    game_name: Option<&str>,
    global: &Global,
    grp: &Group,
    game: &Game,
) -> Result<Effective> {
    let mut env = grp.env.clone();
    env.extend_from_slice(game.env.as_slice());
    let mut drives = vec![];
    for d in &grp.drives {
        match parse_drive_mapping(d) {
            Some(mapping) => drives.push(mapping),
            None => eprintln!("warning: ignoring unparseable drive entry: {d:?}"),
        }
    }

    let mut keywords = global.keywords.clone().unwrap_or_default();
    keywords.extend_from_slice(grp.keywords.as_slice());
    keywords.extend_from_slice(game.keywords.as_slice());
    keywords.extend_from_slice(&["umuw".to_owned(), grp_name.to_owned()]);
    if let Some(n) = game_name {
        keywords.push(n.to_owned());
    }
    // remove duplicate
    keywords.sort();
    keywords.dedup();

    Ok(Effective {
        store: first(&[&game.store, &grp.store]),
        gameid: resolved_gameid(game.gameid.as_deref(), game_name.unwrap_or("default")),
        prefix_fragment: game.prefix.as_deref().unwrap_or(grp_name).to_owned(),
        runner: first(&[&grp.runner, &global.runner]).unwrap_or_else(|| DEFAULT_RUNNER.to_owned()),
        gamescope_opts: first(&[
            &game.gamescope_opts,
            &grp.gamescope_opts,
            &global.gamescope_opts,
        ])
        .unwrap_or_else(default_gamescope_option),
        gamescope_mode: first(&[
            &game.gamescope_mode,
            &grp.gamescope_mode,
            &global.gamescope_mode,
        ])
        .unwrap_or(GamescopeMode::Prompt),
        hdr: first(&[&game.hdr, &grp.hdr, &global.hdr]),
        env,
        app_entry: game.app_entry.clone(),
        wrapper: first(&[&game.wrapper, &grp.wrapper, &global.wrapper]),
        drives,
        keywords,
        args: game.args.clone(),
    })
}

pub fn default_gamescope_option() -> Vec<String> {
    vec![
        "3840 2160 3840 2160 120 hdr vrr".to_owned(),
        "3840 2160 3840 2160 120".to_owned(),
    ]
}
pub enum UpdateGroupMembersRes {
    Success(u32),
    Fail((u32, u32)),
}

pub fn update_group_members(
    config_root: &Path,
    from: &str,
    to: &str,
) -> Result<UpdateGroupMembersRes> {
    let mut total = 0;
    let mut failed = 0;
    for (game_str, mut game) in load_all::<Game>(config_root)? {
        if game.group.as_deref() == Some(from) {
            total += 1;
            game.group = Some(to.to_string());
            if let Err(e) = game.save(config_root, &game_str) {
                eprintln!("unable to update game {game_str}: {e}");
                failed += 1;
            };
        }
    }
    if failed == 0 {
        Ok(UpdateGroupMembersRes::Fail((total - failed, total)))
    } else {
        Ok(UpdateGroupMembersRes::Success(total))
    }
}

fn first<T: Clone>(args: &[&Option<T>]) -> Option<T> {
    // helper to clean-up fallback code in the order of preference a > b > c
    let mut ret = None;
    for arg in args {
        ret = ret.or_else(|| (*arg).clone());
    }
    ret
}

fn rename_toml<'a, T: SaveableToml>(
    config_root: &Path,
    from: &'a str,
    to: &'a str,
) -> Result<&'a str> {
    let from_path = T::toml_path(config_root, from);
    let to_path = T::toml_path(config_root, to);
    if to_path.exists() {
        bail!("{} {to} already exists!", type_name::<T>());
    }
    fs::rename(from_path, to_path)
        .with_context(|| format!("renaming {} {from} to {to}", type_name::<T>()))?;
    Ok(to)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};

    use super::*;
    const DEFAULT_GAMEID: &str = "umu-default";

    fn default_global_none() -> Global {
        Global::default()
    }

    fn default_global_some() -> Global {
        Global {
            gamescope_opts: Some(vec!["gamescope-global".to_owned()]),
            hdr: Some(false),
            runner: Some("runner_global".to_owned()),
            gamescope_mode: Some(GamescopeMode::Native),
            wrapper: Some("global_wrapper".to_owned()),
            keywords: Some(vec!["kw1".to_owned()]),
        }
    }

    fn default_group_none() -> Group {
        Group::default()
    }

    fn default_group_some() -> Group {
        Group {
            drives: vec!["p:/asdf/asdf".to_owned()],
            env: vec!["test_env=1".to_owned()],
            gamescope_opts: Some(vec!["group_gamescope".to_owned()]),
            gamescope_mode: Some(GamescopeMode::Gamescope),
            hdr: Some(false),
            runner: Some("proton-grp".to_owned()),
            wrapper: Some("group-wrapper".to_owned()),
            ..Default::default()
        }
    }

    fn default_game_none() -> Game {
        Game::default()
    }

    fn default_game_some() -> Game {
        Game {
            group: Some("some-grp".to_owned()),
            app_entry: Some("game_exe".to_owned()),
            gamescope_opts: Some(vec!["game-gamescope".to_owned()]),
            gamescope_mode: Some(GamescopeMode::Gamescope),
            hdr: Some(false),
            env: vec!["game_env=1".to_owned()],
            wrapper: Some("game-wrapper".to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn default_if_unset() {
        let res = resolve_effective(
            "default_name",
            None,
            &default_global_none(),
            &default_group_none(),
            &default_game_none(),
        )
        .unwrap();

        assert_eq!(res.runner, DEFAULT_RUNNER);
        assert_eq!(res.gameid, DEFAULT_GAMEID);
        assert_eq!(res.gamescope_opts, default_gamescope_option());
        assert_eq!(res.hdr, None);
        assert!(res.env.is_empty());
        assert_eq!(res.app_entry, None);
        assert_eq!(res.wrapper, None);
    }

    #[test]
    fn env_is_additive() {
        let mut grp = default_group_none();
        let mut game = default_game_none();

        grp.env = vec!["A=1".to_string(), "B=2".to_string()];
        game.env = vec!["C=3".to_string()];

        let res = resolve_effective("", None, &default_global_some(), &grp, &game).unwrap();

        assert_eq!(
            res.env,
            vec!["A=1".to_owned(), "B=2".to_owned(), "C=3".to_owned()]
        );
    }

    #[test]
    fn fallback_to_first_seen() {
        let res = resolve_effective(
            "some ignored name",
            None,
            &default_global_some(),
            &default_group_some(),
            &default_game_some(),
        )
        .unwrap();

        assert_eq!(res.runner, "proton-grp");
        assert_eq!(res.gameid, DEFAULT_GAMEID);
        assert_eq!(res.gamescope_opts, vec!["game-gamescope".to_owned()]);
        assert_eq!(res.hdr, Some(false));
        assert_eq!(
            res.env,
            vec!["test_env=1".to_owned(), "game_env=1".to_owned()]
        );
        assert_eq!(res.app_entry, Some("game_exe".to_owned()));
        assert_eq!(res.wrapper, default_game_some().wrapper);

        // checks keyword is combined
        let mut keywords_expected = default_global_some().keywords.unwrap().clone();
        keywords_expected.append(&mut default_group_some().keywords);
        keywords_expected.append(&mut default_game_some().keywords);
        keywords_expected.push("some ignored name".into());
        keywords_expected.push("umuw".into());
        keywords_expected.sort();
        keywords_expected.dedup();
        assert_eq!(res.keywords, keywords_expected);
    }

    #[test]
    fn resolve_effective_removes_dup_keywords() {
        let res = resolve_effective(
            "game1",
            None,
            &default_global_some(),
            &default_group_some(),
            &Game {
                keywords: vec!["game1".into(), "kywd".into()],
                ..Default::default()
            },
        )
        .unwrap();

        // checks keyword is combined
        let mut keywords_expected = default_global_some().keywords.unwrap().clone();
        keywords_expected.append(&mut default_group_some().keywords);
        keywords_expected.push("kywd".into());
        keywords_expected.push("game1".into());
        keywords_expected.push("umuw".into());
        keywords_expected.sort();
        keywords_expected.dedup();
        assert_eq!(res.keywords, keywords_expected);
    }

    #[test]
    fn resolve_effective_adds_umuw_keyword() {
        let res = resolve_effective(
            "game1",
            None,
            &Global::default(),
            &Group::default(),
            &Game::default(),
        )
        .unwrap();

        // checks keyword is combined
        let mut keywords_expected = vec!["umuw".to_owned(), "game1".to_owned()];
        keywords_expected.sort();
        assert_eq!(res.keywords, keywords_expected);
    }

    #[test]
    fn group_overrides_global() {
        let res = resolve_effective(
            "",
            None,
            &default_global_some(),
            &default_group_some(),
            &default_game_none(),
        )
        .unwrap();
        assert_eq!(res.runner, "proton-grp");
        assert_eq!(res.gamescope_opts, vec!["group_gamescope".to_owned()]);
        assert_eq!(res.hdr, Some(false));
        assert_eq!(res.wrapper, default_group_some().wrapper);
    }

    #[test]
    fn game_overrides_group() {
        let res = resolve_effective(
            "",
            None,
            &default_global_none(),
            &default_group_some(),
            &default_game_some(),
        )
        .unwrap();
        assert_eq!(res.gamescope_opts, vec!["game-gamescope".to_owned()]);
        assert_eq!(res.wrapper, default_game_some().wrapper);
    }

    // parsing of toml test

    #[test]
    fn read_non_existent_config_is_err() {
        let path = Path::new("asdf/asdf/asdfasdfasdfasdfasdfasdf");
        let err = format! {"{:?}", load_profile::<Group>(path).unwrap_err()};

        assert!(err.to_string().contains(&path.display().to_string()));
        assert!(err.to_string().contains("No such file"))
    }

    #[test]
    fn read_invalid_config_is_err() -> Result<()> {
        let mut tmp_config = NamedTempFile::new()?;
        let _ = writeln!(tmp_config, "ofasi;jdjiofjiofasdijoafsd=asdfasdf");
        let err = format! {"{:?}", load_profile::<Group>(tmp_config.path()).unwrap_err()};

        assert!(err.to_string().contains("invalid"));
        assert!(err.to_string().contains("Parsing file"));
        assert!(
            err.to_string()
                .contains(tmp_config.path().to_str().unwrap())
        );

        Ok(())
    }

    #[test]
    fn read_valid_config_ok() -> Result<()> {
        let mut tmp_config = NamedTempFile::new()?;
        let _ = writeln!(
            tmp_config,
            "group = 'umu-gamegroup'\ngamescope_mode = 'prompt'"
        );
        let game = load_profile::<Game>(tmp_config.path()).unwrap();

        assert_eq!(game.group, Some("umu-gamegroup".to_owned()));
        assert!(game.env.is_empty());
        assert_eq!(game.gamescope_mode, Some(GamescopeMode::Prompt));
        assert_eq!(game.gamescope_opts, None);
        assert_eq!(game.hdr, None);

        Ok(())
    }

    #[test]
    fn read_group_toml_is_correct() {
        let grp = toml::from_str::<Group>("gameid = 'umu-foo'").unwrap();
        assert!(grp.drives.is_empty());
        assert!(grp.env.is_empty());
        assert_eq!(grp.gamescope_opts, None);
        assert_eq!(grp.gamescope_mode, None);
        assert_eq!(grp.hdr, None);
        assert_eq!(grp.runner, None);
    }

    #[test]
    fn read_game_toml_is_correct() {
        let game =
            toml::from_str::<Game>("group = 'umu-gamegroup'\ngamescope_mode = 'prompt'").unwrap();
        assert_eq!(game.group, Some("umu-gamegroup".to_owned()));
        assert!(game.env.is_empty());
        assert_eq!(game.gamescope_mode, Some(GamescopeMode::Prompt));
        assert_eq!(game.gamescope_opts, None);
        assert_eq!(game.hdr, None);
    }

    #[test]
    fn read_global_toml_is_correct() {
        let global = toml::from_str::<Global>("runner = 'proton-foo'").unwrap();
        assert_eq!(global.runner, Some("proton-foo".to_owned()));
        assert_eq!(global.gamescope_mode, None);
        assert_eq!(global.gamescope_opts, None);
        assert_eq!(global.hdr, None);
    }

    #[test]
    fn list_all_profile_toml_obtain_correct_group_profiles() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_dir.path();
        let profile_names = vec!["test1", "test2", "TEST3", "TEST4", "test5"];

        for name in &profile_names {
            Group::seed().save(root, name).unwrap();
        }

        let mut expected_path: Vec<PathBuf> = profile_names
            .iter()
            .map(|name| Group::toml_path(root, name))
            .collect();
        expected_path.sort();

        let mut res = list_all_profile_toml::<Group>(root).unwrap();
        res.sort();

        assert_eq!(res, expected_path);
    }

    #[test]
    fn list_all_profile_toml_obtain_correct_games_profiles() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_dir.path();
        let profile_names = vec!["test1", "test2", "TEST3", "TEST4", "test5"];

        for name in &profile_names {
            Game::default().save(root, name).unwrap();
        }

        let mut expected_path: Vec<PathBuf> = profile_names
            .iter()
            .map(|name| Game::toml_path(root, name))
            .collect();
        expected_path.sort();

        let mut res = list_all_profile_toml::<Game>(root).unwrap();
        res.sort();

        assert_eq!(res, expected_path);
    }

    #[test]
    fn load_all_loads_all_groups() {
        let tmp_config = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_config.path();

        let mut profile_names = vec!["test1", "test2", "TEST3", "TEST4", "test5"];
        profile_names.sort();
        let mut groups = vec![];
        for profile_name in profile_names {
            let grp = Group {
                runner: Some(profile_name.to_owned()),
                ..Default::default()
            };
            grp.save(root, profile_name).unwrap();
            groups.push(grp);
        }
        let res: Vec<Group> = load_all::<Group>(root).unwrap().into_values().collect();
        assert_eq!(groups, res);
    }

    #[test]
    fn load_all_loads_all_games() {
        let tmp_config = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_config.path();

        let mut profile_names = vec!["test1", "test2", "TEST3", "TEST4", "test5"];
        profile_names.sort();
        let mut games = vec![];
        for profile_name in profile_names {
            let game = Game {
                app_entry: Some(profile_name.to_owned()),
                ..Default::default()
            };
            game.save(root, profile_name).unwrap();
            games.push(game);
        }
        let res: Vec<Game> = load_all::<Game>(root).unwrap().into_values().collect();
        assert_eq!(games, res);
    }

    #[test]
    fn load_all_warns_on_invalid_profiles() {
        let tmp_config = TempDir::new().context("creating tmp config dir").unwrap();
        let root = tmp_config.path();

        Group::seed().save(root, "good").unwrap();
        fs::write(Group::dir(root).join("broken.toml"), "not toml ][ at all").unwrap();

        let all = load_all::<Group>(root).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all.contains_key("good"));
    }

    #[test]
    fn resolve_effective_fallsback_gameid_when_unset() {
        let grp_name = "group-frag-fallback-test";
        let game_name = "gameid-fallback-test";
        let eff = resolve_effective(
            grp_name,
            Some(game_name),
            &Global::default(),
            &Group::default(),
            &Game::default(),
        )
        .unwrap();

        assert_eq!(eff.gameid, "umu-gameid-fallback-test");
    }

    #[test]
    fn resolve_effective_fallsback_prefix_frag() {
        let grp_name = "group-frag-fallback-test";
        let game_name = "gameid-fallback-test";
        let eff = resolve_effective(
            grp_name,
            Some(game_name),
            &Global::default(),
            &Group::default(),
            &Game::default(),
        )
        .unwrap();

        assert_eq!(eff.prefix_fragment, "group-frag-fallback-test");
    }

    #[test]
    fn resolve_effective_uses_game_prefix_when_set() {
        let grp_name = "group-frag-fallback-test";
        let game_name = "gameid-fallback-test";
        let eff = resolve_effective(
            grp_name,
            Some(game_name),
            &Global::default(),
            &Group::default(),
            &Game {
                prefix: Some("example-prefix".to_owned()),
                ..Game::default()
            },
        )
        .unwrap();

        assert_eq!(eff.prefix_fragment, "example-prefix");
    }

    #[test]
    fn dmenu_preference_parses_lowercase_and_rejects_invalid() {
        let s: Setting = toml::from_str("dmenu = 'rofi'").unwrap();
        assert_eq!(s.dmenu, DmenuPreference::Rofi);
        assert!(toml::from_str::<Setting>("dmenu = 'noctalia'").is_ok());
        assert!(toml::from_str::<Setting>("dmenu = 'bemenu'").is_err());
    }

    #[test]
    fn umuw_config_saves_load_correctly() {
        let config = UmuwConfig {
            setting: Some(Setting {
                dmenu: DmenuPreference::Rofi,
                ..Default::default()
            }),
            global: Some(Global {
                wrapper: Some("test".to_owned()),
                ..Default::default()
            }),
        };
        let dir = TempDir::new().unwrap();
        config.save(dir.path(), "umuw").unwrap();
        let loaded = UmuwConfig::load(dir.path(), "umuw").unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn setting_save_preserves_global_section() {
        let dir = TempDir::new().unwrap();
        Global {
            runner: Some("proton-1234".into()),
            ..Default::default()
        }
        .save(dir.path(), "umuw")
        .unwrap();

        Setting {
            dmenu: DmenuPreference::Rofi,
            ..Default::default()
        }
        .save(dir.path(), "umuw")
        .unwrap();

        let reloaded = UmuwConfig::load(dir.path(), "umuw").unwrap();
        assert_eq!(
            reloaded.global.unwrap().runner,
            Some("proton-1234".to_owned())
        );
        assert_eq!(reloaded.setting.unwrap().dmenu, DmenuPreference::Rofi);

        let global = Global::load(dir.path(), "umuw").unwrap();
        assert_eq!(global.runner, Some("proton-1234".to_owned()));
    }

    #[test]
    fn global_save_preserves_setting_section() {
        let dir = TempDir::new().unwrap();
        Setting {
            dmenu: DmenuPreference::Rofi,
            ..Default::default()
        }
        .save(dir.path(), "umuw")
        .unwrap();

        Global {
            runner: Some("proton-1234".into()),
            ..Default::default()
        }
        .save(dir.path(), "umuw")
        .unwrap();

        let reloaded = UmuwConfig::load(dir.path(), "umuw").unwrap();
        assert_eq!(reloaded.setting.unwrap().dmenu, DmenuPreference::Rofi);

        let setting = Setting::load(dir.path(), "umuw").unwrap();
        assert_eq!(setting.dmenu, DmenuPreference::Rofi);
    }

    #[test]
    fn group_rename_refuses_default() {
        let config_dir = TempDir::new().unwrap();
        let config_root = config_dir.path();
        let default_path = Group::default_path(config_root);
        Group::seed()
            .save(config_root, Group::DEFAULT_CONFIG)
            .unwrap();
        assert!(default_path.exists());
        let res = Group::rename(config_root, Group::DEFAULT_CONFIG, "some other name");
        assert!(res.is_err_and(|e| e.to_string().contains("default group")));
    }

    #[test]
    fn rename_refuses_existing_groups() {
        let config_dir = TempDir::new().unwrap();
        let config_root = config_dir.path();
        Group::seed().save(config_root, "grp1").unwrap();
        Group::seed().save(config_root, "grp2").unwrap();
        let res = Group::rename(config_root, "grp1", "grp2");
        assert!(res.is_err_and(|e| e.to_string().contains("already exists!")));
    }

    #[test]
    fn rename_renames_file() {
        let config_dir = TempDir::new().unwrap();
        let config_root = config_dir.path();
        Game::seed().save(config_root, "g1").unwrap();
        assert!(Game::toml_path(config_root, "g1").exists());
        Game::rename(config_root, "g1", "g2").unwrap();
        assert!(!Game::toml_path(config_root, "g1").exists());
        assert!(Game::toml_path(config_root, "g2").exists());
    }

    #[test]
    fn update_group_members_updates_relevant_group_names() {
        let config_dir = TempDir::new().unwrap();
        let config_root = config_dir.path();
        Group::seed().save(config_root, "grp1").unwrap();
        let game = Game {
            group: Some("grp1".to_owned()),
            ..Default::default()
        };
        let gs = ["g1", "g2", "g3"];
        for g in gs {
            game.save(config_root, g).unwrap();
        }
        Game {
            group: Some("unaffected group".into()),
            ..Default::default()
        }
        .save(config_root, "g4")
        .unwrap();

        for g in gs {
            assert!(Game::toml_path(config_root, g).exists());
        }
        assert!(Game::toml_path(config_root, "g4").exists());
        update_group_members(config_root, "grp1", "grp2").unwrap();
        for g in gs {
            assert_eq!(Game::load(config_root, g).unwrap().group.unwrap(), "grp2");
        }
        assert_eq!(
            Game::load(config_root, "g4").unwrap().group,
            Some("unaffected group".into())
        );
    }
}
