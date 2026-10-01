use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tabular::{Table, row, table};

use crate::{
    config::{self, FallbackPath, Game, Global, Group, resolve_effective, resolved_gameid},
    prefix::{self, prefix_path},
};

pub fn list_profiles(config_root: &Path, home: &Path) -> Result<Table> {
    let groups = config::load_all::<Group>(config_root)?;
    let games = config::load_all::<Game>(config_root)?;
    let mut group_games = BTreeMap::<&str, Vec<&str>>::new();

    for (game_name, game) in &games {
        let group_name = game.group.as_deref().unwrap_or(Group::DEFAULT_CONFIG);
        group_games.entry(group_name).or_default().push(game_name);
    }

    // GROUP   GAME   GAMEID   PREFIX   EXISTS
    let mut table_list = table!(
        "{:<}   {:^}    {:^}    {:^}    {:<}    {:<}    {:<}",
        row!(
            "GROUP",
            "GAME",
            "STORE",
            "GAMEID",
            "PREFIX",
            "EXISTS",
            "PREFIX_SIZE"
        )
    );

    let mut prefix_size_map = HashMap::<&str, String>::new();

    let global = Global::default();
    for (group_name, group) in &groups {
        let prefix_location = prefix_path(group_name, home);
        let prefix_size = prefix_size_map
            .entry(group_name)
            .or_insert_with(|| prefix::get_prefix_size(&prefix_location));

        let game_names = group_games
            .remove(group_name.as_str())
            .map(|mut names| {
                names.sort();
                names
            })
            .unwrap_or_else(|| vec!["-"]);

        for game_name in game_names {
            if let Some(game) = games.get(game_name) {
                let eff = resolve_effective(group_name, Some(game_name), &global, group, game)?;
                let tmp_prefix = game
                    .prefix
                    .as_ref()
                    .map(|pre| prefix::prefix_path(pre, home));
                let size = match &tmp_prefix {
                    Some(p) => &prefix::get_prefix_size(p),
                    None => prefix_size,
                };
                let tmp_prefix = tmp_prefix.as_ref().unwrap_or(&prefix_location);
                table_list.add_row(row!(
                    &group_name,
                    game_name,
                    eff.store.as_deref().unwrap_or("default"),
                    eff.gameid,
                    tmp_prefix.display(),
                    tmp_prefix.exists(),
                    size
                ));
            } else {
                table_list.add_row(row!(
                    &group_name,
                    game_name,
                    group.store.as_deref().unwrap_or("default"),
                    "?",
                    prefix_location.display(),
                    prefix_location.exists(),
                    &prefix_size
                ));
            }
        }
    }

    if !group_games.is_empty() {
        table_list.add_heading("\n--- non-existent group ---");
        for (deadgroup, mut game_names) in group_games {
            game_names.sort();
            for game_name in game_names {
                let game = games.get(game_name).context("getting game from map")?;
                let game_prefix = game
                    .prefix
                    .as_deref()
                    .map(|prefix| prefix::prefix_path(prefix, home))
                    .unwrap_or(PathBuf::from("-"));
                let prefix_size = prefix::get_prefix_size(&game_prefix);

                table_list.add_row(row!(
                    &deadgroup,
                    game_name,
                    game.store.as_deref().unwrap_or("default"),
                    resolved_gameid(game.gameid.as_deref(), game_name),
                    game_prefix.display(),
                    game_prefix.exists(),
                    &prefix_size
                ));
            }
        }
    }

    Ok(table_list)
}

#[cfg(test)]
mod tests {
    use anyhow::Context;
    use tempfile::TempDir;

    use crate::{config::SaveableToml, paths};

    use super::*;

    fn row_containing<'a>(table: &'a str, needle: &str) -> &'a str {
        table
            .lines()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("no row contains {needle} in {table}!"))
    }

    #[test]
    fn list_profiles_game_row_pair_w_group() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let home_dir = TempDir::new().context("creating tmp home dir").unwrap();
        let root = tmp_dir.path();
        let home = home_dir.path();
        paths::initialize_config(root).unwrap();
        let grp1 = Group {
            ..Default::default()
        };

        grp1.save(root, "example").unwrap();

        let g1 = Game {
            group: Some("example".to_owned()),
            ..Default::default()
        };

        g1.save(root, "game1").unwrap();

        let s = list_profiles(root, home).unwrap().to_string();
        let row = row_containing(&s, "game1");
        assert!(row.contains("umu-game1"));
        assert!(row.contains("false"));
    }

    #[test]
    fn list_profiles_non_existant_grp_in_dead_section() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let home_dir = TempDir::new().context("creating tmp home dir").unwrap();
        let root = tmp_dir.path();
        let home = home_dir.path();
        paths::initialize_config(root).unwrap();
        let grp1 = Group {
            ..Default::default()
        };

        grp1.save(root, "example").unwrap();

        let g2 = Game {
            group: Some("non-existant-group".to_owned()),
            ..Default::default()
        };

        g2.save(root, "game2").unwrap();

        let s = list_profiles(root, home).unwrap().to_string();

        assert!(s.contains("non-existent group"));
        let row = row_containing(&s, "game2");
        assert!(row.contains("umu-game2"));
    }

    #[test]
    fn list_profiles_gameless_grp_shows_placeholder() {
        let tmp_dir = TempDir::new().context("creating tmp config dir").unwrap();
        let home_dir = TempDir::new().context("creating tmp home dir").unwrap();
        let root = tmp_dir.path();
        let home = home_dir.path();
        paths::initialize_config(root).unwrap();

        let s = list_profiles(root, home).unwrap().to_string();
        assert!(row_containing(&s, "default").contains("-"));
        assert!(!s.contains("non-existent group"));
        assert_eq!(s.lines().count(), 2)
    }
}
