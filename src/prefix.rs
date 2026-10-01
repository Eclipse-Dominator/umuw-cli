use std::{
    fmt::Display,
    fs::{self},
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::prefix::{
    DriveVerifyResult::InvalidLetter,
    TransferPrefixRes::{SrcMissing, TargetExist},
};

const PRESERVED_DRIVE: &[char] = &['c', 'z'];

#[derive(Clone, PartialEq, Debug)]
pub struct DriveMapping {
    letter: char,
    src: PathBuf,
}

#[derive(PartialEq, Debug)]
pub struct VerifiedDriveMapping<'a> {
    letter: char,
    src: &'a Path,
}

#[derive(PartialEq, Debug)]
pub enum DriveVerifyResult<'a> {
    InvalidSrcNonExist(&'a Path),
    InvalidSrcRelative(&'a Path),
    InvalidLetter(char),
    InvalidPrefix(&'a Path),
    Success(VerifiedDriveMapping<'a>),
}

#[derive(PartialEq, Debug)]
pub enum LinkingResult {
    LnLocationInvalid(PathBuf),
    Repointed,
    AlreadyOk,
    Created,
}

#[derive(PartialEq, Debug)]
pub enum EnsureDrivesRes {
    InvalidPrefix(PathBuf),
    Complete,
    CompleteWithFailure,
}

impl DriveMapping {
    pub fn verify<'a>(&'a self, prefix: &'a Path) -> DriveVerifyResult<'a> {
        if !self.letter.is_ascii_alphabetic()
            || self.letter.is_uppercase()
            || PRESERVED_DRIVE.contains(&self.letter)
        {
            return InvalidLetter(self.letter);
        }
        if !self.src.exists() {
            return DriveVerifyResult::InvalidSrcNonExist(&self.src);
        }
        if self.src.is_relative() {
            return DriveVerifyResult::InvalidSrcRelative(&self.src);
        }
        let symlink_target = prefix.join("dosdevices");
        if !symlink_target.exists() {
            return DriveVerifyResult::InvalidPrefix(prefix);
        }
        DriveVerifyResult::Success(VerifiedDriveMapping {
            letter: self.letter,
            src: &self.src,
        })
    }
}

impl<'a> VerifiedDriveMapping<'a> {
    pub fn ensure_symlink(&self, prefix: &Path) -> Result<LinkingResult> {
        let symlink_target = prefix.join("dosdevices").join(format!("{}:", self.letter));
        let mut is_repoint = false;
        if symlink_target.exists() {
            if !symlink_target.is_symlink() {
                return Ok(LinkingResult::LnLocationInvalid(symlink_target));
            }

            let ln_loc = symlink_target.read_link().context("obtaining link path")?;
            if ln_loc == self.src {
                return Ok(LinkingResult::AlreadyOk);
            }
            eprintln!("warning: existing ln to {}, repointing", ln_loc.display());
            fs::remove_file(&symlink_target).context("removing stale symlink")?;
            is_repoint = true;
        }
        symlink(self.src, &symlink_target).with_context(|| {
            format!(
                "Linking {} to {}",
                self.src.display(),
                symlink_target.display()
            )
        })?;
        match is_repoint {
            false => Ok(LinkingResult::Created),
            true => Ok(LinkingResult::Repointed),
        }
    }
}

impl Display for DriveMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.letter, self.src.display())
    }
}

#[derive(PartialEq, Debug)]
pub enum TransferPrefixRes {
    Success,
    TargetExist,
    SrcMissing,
}

pub fn transfer_prefix(frag1: &str, frag2: &str, home: &Path) -> Result<TransferPrefixRes> {
    let path1 = prefix_path(frag1, home);
    let path2 = prefix_path(frag2, home);
    if !path1.exists() {
        return Ok(SrcMissing);
    }
    if path2.exists() {
        return Ok(TargetExist);
    }

    fs::rename(&path1, &path2).with_context(|| {
        format!(
            "transfering prefix: {frag1}:{} => {frag2}:{}",
            path1.display(),
            path2.display()
        )
    })?;
    Ok(TransferPrefixRes::Success)
}

pub fn parse_drive_mapping(s: &str) -> Option<DriveMapping> {
    let (letter, entry) = s.split_once(":")?;
    let Ok(letter) = letter.parse::<char>() else {
        return None;
    };
    Some(DriveMapping {
        letter,
        src: PathBuf::from(shellexpand::tilde(entry).as_ref()),
    })
}

pub fn prefix_path(fragment: &str, home: &Path) -> PathBuf {
    let expanded = shellexpand::tilde(fragment);
    if Path::new(expanded.as_ref()).is_absolute() {
        PathBuf::from(expanded.as_ref())
    } else {
        home.join("Games").join("umuw").join(fragment)
    }
}

pub fn ensure_drives(prefix: &Path, drives: &[DriveMapping]) -> Result<EnsureDrivesRes> {
    let mut failed_drives = vec![];
    for drive in drives {
        match drive.verify(prefix) {
            DriveVerifyResult::Success(verified) => match verified.ensure_symlink(prefix)? {
                LinkingResult::LnLocationInvalid(path_buf) => {
                    eprintln!("symlink cannot be created at {}!", path_buf.display());
                    failed_drives.push(drive);
                }
                LinkingResult::Repointed => println!("stale symlink detected, repointing"),
                LinkingResult::AlreadyOk | LinkingResult::Created => {}
            },
            DriveVerifyResult::InvalidPrefix(s) => {
                return Ok(EnsureDrivesRes::InvalidPrefix(s.into()));
            }
            DriveVerifyResult::InvalidSrcNonExist(s) => {
                eprintln!("warning: src path {} is invalid!", s.display());
                failed_drives.push(drive);
            }
            DriveVerifyResult::InvalidSrcRelative(s) => {
                eprintln!("warning: src path {} cannot be relative!", s.display());
                failed_drives.push(drive);
            }
            DriveVerifyResult::InvalidLetter(c) => {
                eprintln!("warning: drive letter {}: is invalid!", c);
                failed_drives.push(drive);
            }
        }
    }

    if failed_drives.is_empty() {
        Ok(EnsureDrivesRes::Complete)
    } else {
        eprintln!("The following drive failed to mount, expect potential errors when launching:");
        for (i, failed_drive) in failed_drives.iter().enumerate() {
            eprintln!("{}. {}", i + 1, failed_drive);
        }
        Ok(EnsureDrivesRes::CompleteWithFailure)
    }
}

pub fn get_prefix_size(prefix: &Path) -> String {
    if !prefix.exists() {
        return "-".to_string();
    }

    match get_folder_size(prefix) {
        Ok(size) => format!("{:.2} GiB", (size as f64) / 1_073_741_824.0),
        Err(e) => {
            eprintln!("warning: sizing {}: {e:#}", prefix.display());
            "-".to_owned()
        }
    }
}

fn get_folder_size(folder: &Path) -> Result<u64> {
    let mut ret_size: u64 = 0;

    for entry in
        fs::read_dir(folder).with_context(|| format!("Reading dir {}", folder.display()))?
    {
        let entry = entry.with_context(|| format!("Obtaining file in {}", folder.display()))?;
        let metadata = fs::symlink_metadata(entry.path())
            .with_context(|| format!("Obtaining metadata in {}", entry.path().display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            ret_size += get_folder_size(&entry.path())?;
        } else {
            ret_size += metadata.len()
        }
    }

    Ok(ret_size)
}

#[cfg(test)]
mod tests {
    use core::panic;
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn prefix_path_returns_correct_path_depending_on_store() {
        let dummy_home = Path::new("/dummy_home");
        let res = prefix_path("demogroup", dummy_home);
        assert_eq!(res, dummy_home.join("Games").join("umuw").join("demogroup"));
    }

    #[test]
    fn folder_size_sums_files_and_skips_symlinks() {
        let t = TempDir::new().unwrap();
        fs::write(t.path().join("a"), vec![0; 100]).unwrap();
        fs::write(t.path().join("b"), vec![0; 50]).unwrap();
        fs::create_dir(t.path().join("sub")).unwrap();
        fs::write(t.path().join("sub").join("c"), vec![0; 25]).unwrap();
        symlink("/mnt/ssd/GAMES", t.path().join("dosdevices")).unwrap();

        assert_eq!(get_folder_size(t.path()).unwrap(), 175);
        assert_eq!(get_prefix_size(&t.path().join("nope")), "-");
    }

    #[test]
    fn parse_drive_mapping_correctly_absolutize_entry() {
        let entry: &str = "harro/someapplicationfolder";
        let res = parse_drive_mapping(&format!("d:{}", entry)).unwrap();
        assert_eq!(
            res,
            DriveMapping {
                letter: 'd',
                src: entry.into(),
            },
        );
    }

    #[test]
    fn parse_drive_mapping_correctly_removes_colons_partitions() {
        let entry: &str = "harro/someapplicationfolder";
        let res = parse_drive_mapping(&format!("z:{}", entry)).unwrap();
        assert_eq!(
            res,
            DriveMapping {
                letter: 'z',
                src: entry.into(),
            },
        );
    }

    #[test]
    fn parse_drive_mapping_correctly_removes_trailing_slashes() {
        let entry: &str = "harro/someapplicationfolder/";
        let res = parse_drive_mapping(&format!("d:{}", entry)).unwrap();
        assert_eq!(
            res,
            DriveMapping {
                letter: 'd',
                src: entry.into(),
            },
        );
    }

    #[test]
    fn parse_drive_mapping_reject_non_char_letter() {
        let entry = TempDir::new().unwrap();
        let test = parse_drive_mapping(&format!("dfail:{}", entry.path().display()));
        assert!(test.is_none());
    }

    #[test]
    fn parse_drive_mapping_expand_tilde() {
        let test = parse_drive_mapping("d:~/Game/test").unwrap();
        assert_eq!(
            test.src,
            PathBuf::from(shellexpand::tilde("~/Game/test").to_string())
        );
    }

    #[test]
    fn verify_reject_invalid_entry() {
        let prefix = TempDir::new().unwrap();
        fs::create_dir(prefix.path().join("dosdevices")).unwrap();
        let res = parse_drive_mapping("d:/some/non/existent").unwrap();
        let res = res.verify(prefix.path());
        assert_eq!(
            res,
            DriveVerifyResult::InvalidSrcNonExist(Path::new("/some/non/existent"))
        )
    }

    #[test]
    fn verify_reject_path_with_non_letter() {
        let prefix = TempDir::new().unwrap();
        fs::create_dir(prefix.path().join("dosdevices")).unwrap();
        let entry = TempDir::new().unwrap();
        let test = parse_drive_mapping(&format!("5:{}", entry.path().display())).unwrap();
        let verified = test.verify(prefix.path());
        assert_eq!(verified, DriveVerifyResult::InvalidLetter('5'));
    }

    #[test]
    fn verify_reject_path_with_c_drive() {
        let prefix = TempDir::new().unwrap();
        fs::create_dir(prefix.path().join("dosdevices")).unwrap();
        let entry = TempDir::new().unwrap();
        let test = parse_drive_mapping(&format!("c:{}", entry.path().display())).unwrap();
        let verified: DriveVerifyResult<'_> = test.verify(prefix.path());
        assert_eq!(verified, DriveVerifyResult::InvalidLetter('c'));
    }

    #[test]
    fn verify_and_ensure_correctly_parses_correct_path() {
        let prefix = TempDir::new().unwrap();
        fs::create_dir(prefix.path().join("dosdevices")).unwrap();
        let entry = TempDir::new().unwrap();
        let test = parse_drive_mapping(&format!("d:{}", entry.path().display())).unwrap();
        let verified = test.verify(prefix.path());
        let expected = prefix.path().join("dosdevices").join("d:");
        assert_eq!(
            verified,
            DriveVerifyResult::Success(VerifiedDriveMapping {
                letter: 'd',
                src: entry.path()
            })
        );
        assert!(!expected.exists());
        let DriveVerifyResult::Success(verified) = verified else {
            panic!("cant be reached")
        };
        verified.ensure_symlink(prefix.path()).unwrap();
        assert!(expected.exists());
    }

    #[test]
    fn ensure_creates_link() {
        let prefix = TempDir::new().unwrap();
        fs::create_dir(prefix.path().join("dosdevices")).unwrap();
        let entry = TempDir::new().unwrap();
        let ln_path = prefix.path().join("dosdevices").join("d:");
        let test = parse_drive_mapping(&format!("d:{}", entry.path().display())).unwrap();
        let DriveVerifyResult::Success(mapping) = test.verify(prefix.path()) else {
            panic!("verify failure!");
        };

        assert!(!ln_path.exists());
        let res = mapping.ensure_symlink(prefix.path()).unwrap();
        assert!(ln_path.exists());
        assert_eq!(res, LinkingResult::Created);
        assert_eq!(ln_path.read_link().unwrap(), entry.path().to_path_buf());
    }

    #[test]
    fn ensure_overwrites_stale_link() {
        let prefix = TempDir::new().unwrap();
        fs::create_dir(prefix.path().join("dosdevices")).unwrap();
        let entry = TempDir::new().unwrap();
        let entry_stale = TempDir::new().unwrap();
        let ln_path = prefix.path().join("dosdevices").join("d:");
        let test = parse_drive_mapping(&format!("d:{}", entry.path().display())).unwrap();
        let DriveVerifyResult::Success(mapping) = test.verify(prefix.path()) else {
            panic!("verify failure!");
        };
        symlink(entry_stale.path(), &ln_path).unwrap();
        let res = mapping.ensure_symlink(prefix.path()).unwrap();
        assert!(ln_path.exists());
        assert_eq!(ln_path.read_link().unwrap(), entry.path().to_path_buf());
        assert_eq!(res, LinkingResult::Repointed);
    }

    #[test]
    fn ensure_ignore_correct_link() {
        let prefix = TempDir::new().unwrap();
        fs::create_dir(prefix.path().join("dosdevices")).unwrap();
        let entry = TempDir::new().unwrap();
        let ln_path = prefix.path().join("dosdevices").join("y:");
        let test = parse_drive_mapping(&format!("y:{}", entry.path().display())).unwrap();
        let DriveVerifyResult::Success(mapping) = test.verify(prefix.path()) else {
            panic!("verify failure!");
        };
        symlink(entry.path(), &ln_path).unwrap();
        let res = mapping.ensure_symlink(prefix.path()).unwrap();
        assert!(ln_path.exists());
        assert_eq!(ln_path.read_link().unwrap(), entry.path().to_path_buf());
        assert_eq!(res, LinkingResult::AlreadyOk);
    }

    #[test]
    fn ensure_reject_invalid_target() {
        let prefix = TempDir::new().unwrap();
        fs::create_dir(prefix.path().join("dosdevices")).unwrap();
        let entry = TempDir::new().unwrap();
        let ln_path = prefix.path().join("dosdevices").join("d:");
        let test = parse_drive_mapping(&format!("d:{}", entry.path().display())).unwrap();
        let DriveVerifyResult::Success(mapping) = test.verify(prefix.path()) else {
            panic!("verify failure!");
        };
        fs::create_dir(&ln_path).unwrap();
        let res = mapping.ensure_symlink(prefix.path()).unwrap();
        assert_eq!(res, LinkingResult::LnLocationInvalid(ln_path));
    }

    #[test]
    fn transfer_prefix_moves_prefix() {
        let home = TempDir::new().unwrap();
        let ori_dir = home.path().join("prefix").to_str().unwrap().to_string();
        let target_dir = home.path().join("prefix2").to_str().unwrap().to_string();

        fs::create_dir(&ori_dir).unwrap();
        assert_eq!(
            transfer_prefix(&ori_dir, &target_dir, home.path()).unwrap(),
            TransferPrefixRes::Success
        );
        assert!(!Path::new(&ori_dir).exists());
        assert!(Path::new(&target_dir).exists());
    }

    #[test]
    fn transfer_returns_false_when_prefix_missing() {
        let home = TempDir::new().unwrap();
        let ori_dir = home.path().join("prefix").to_str().unwrap().to_string();
        let target_dir = home.path().join("prefix2").to_str().unwrap().to_string();

        assert_eq!(
            transfer_prefix(&ori_dir, &target_dir, home.path()).unwrap(),
            TransferPrefixRes::SrcMissing
        );
    }

    #[test]
    fn transfer_returns_false_when_target_exist() {
        let home = TempDir::new().unwrap();
        let ori_dir = home.path().join("prefix").to_str().unwrap().to_string();
        let target_dir = home.path().join("prefix2").to_str().unwrap().to_string();
        fs::create_dir(&ori_dir).unwrap();
        fs::create_dir(&target_dir).unwrap();

        assert_eq!(
            transfer_prefix(&ori_dir, &target_dir, home.path()).unwrap(),
            TransferPrefixRes::TargetExist
        );
        assert!(Path::new(&ori_dir).exists());
    }
}
