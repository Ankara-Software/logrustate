use anyhow::Result;
use chrono::Local;
use colored::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::compress::compress_file;
use crate::config::{LogConfig, LogEntry};
use crate::state::StateDB;

pub struct Rotator<'a> {
    pub state_db: &'a mut StateDB,
    pub dry_run: bool,
    pub verbose: bool,
    pub force: bool,
}

impl<'a> Rotator<'a> {
    pub fn new(state_db: &'a mut StateDB, dry_run: bool, verbose: bool, force: bool) -> Self {
        Rotator {
            state_db,
            dry_run,
            verbose,
            force,
        }
    }

    pub fn process_entry(&mut self, entry: &LogEntry) -> Result<()> {
        let mut matched_files = Vec::new();
        for path_pattern in &entry.paths {
            if path_pattern.contains('*') || path_pattern.contains('?') {
                if let Ok(paths) = glob::glob(path_pattern) {
                    for p_entry in paths {
                        if let Ok(p) = p_entry
                            && p.is_file()
                        {
                            matched_files.push(p);
                        }
                    }
                }
            } else {
                let p = PathBuf::from(path_pattern);
                if p.exists() && p.is_file() {
                    matched_files.push(p);
                }
            }
        }

        if matched_files.is_empty() {
            if self.verbose {
                println!("No matching files found for pattern {:?}", entry.paths);
            }
            return Ok(());
        }

        let mut files_to_rotate = Vec::new();
        for file in &matched_files {
            if self.should_rotate_file(file, &entry.config)? {
                files_to_rotate.push(file.clone());
            }
        }

        if files_to_rotate.is_empty() {
            if self.verbose {
                println!("No files require rotation in this block.");
            }
            return Ok(());
        }

        if let Some(ref script) = entry.config.prerotate
            && entry.config.sharedscripts == Some(true)
        {
            self.run_script(script, &entry.paths.join(" "))?;
        }

        for file in &files_to_rotate {
            if let Some(ref script) = entry.config.prerotate
                && entry.config.sharedscripts != Some(true)
            {
                self.run_script(script, &file.to_string_lossy())?;
            }

            self.rotate_single_file(file, &entry.config)?;

            if let Some(ref script) = entry.config.postrotate
                && entry.config.sharedscripts != Some(true)
            {
                self.run_script(script, &file.to_string_lossy())?;
            }
        }

        if let Some(ref script) = entry.config.postrotate
            && entry.config.sharedscripts == Some(true)
        {
            self.run_script(script, &entry.paths.join(" "))?;
        }

        Ok(())
    }

    fn should_rotate_file(&self, path: &Path, config: &LogConfig) -> Result<bool> {
        let metadata = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                if config.missingok == Some(true) {
                    return Ok(false);
                }
                return Err(
                    anyhow::Error::new(e).context(format!("Failed to get metadata for {:?}", path))
                );
            }
        };

        let size = metadata.len();
        if size == 0 && config.notifempty == Some(true) {
            if self.verbose {
                println!("{:?} is empty and notifempty is set, skipping.", path);
            }
            return Ok(false);
        }

        if self.force {
            if self.verbose {
                println!("Rotation forced for {:?}", path);
            }
            return Ok(true);
        }

        let path_str = path.to_string_lossy().to_string();

        let time_rotate = if let Some(ref freq) = config.frequency {
            self.state_db.should_rotate(&path_str, freq)
        } else {
            false
        };

        if let Some(s) = config.size
            && size > s
        {
            if self.verbose {
                println!(
                    "{:?} size {} exceeds size threshold {}, rotating.",
                    path, size, s
                );
            }
            return Ok(true);
        }

        if let Some(mins) = config.minsize
            && size > mins
            && time_rotate
        {
            if self.verbose {
                println!(
                    "{:?} size {} exceeds minsize {} and time threshold met, rotating.",
                    path, size, mins
                );
            }
            return Ok(true);
        }

        if let Some(maxs) = config.maxsize
            && (size > maxs || time_rotate)
        {
            if self.verbose {
                println!(
                    "{:?} size {} exceeds maxsize {} or time threshold met, rotating.",
                    path, size, maxs
                );
            }
            return Ok(true);
        }

        if time_rotate {
            if self.verbose {
                println!("{:?} time threshold met, rotating.", path);
            }
            return Ok(true);
        }

        Ok(false)
    }

    fn rotate_single_file(&mut self, path: &Path, config: &LogConfig) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();
        if self.dry_run {
            println!("{} {:?}", "Would rotate".yellow().bold(), path);
            self.state_db.update(&path_str);
            return Ok(());
        }

        let dest_base = if let Some(ref olddir) = config.olddir {
            let p = Path::new(olddir);
            let absolute = if p.is_absolute() {
                p.to_path_buf()
            } else {
                path.parent().unwrap_or_else(|| Path::new("")).join(p)
            };
            fs::create_dir_all(&absolute).ok();
            absolute
        } else {
            path.parent().unwrap_or_else(|| Path::new("")).to_path_buf()
        };

        let file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        if config.dateext == Some(true) {
            if config.rotate == Some(0) {
                self.rotate_without_backups(path, config)?;
            } else {
                let fmt = config.dateformat.as_deref().unwrap_or("-%Y%m%d");
                let date_str = Local::now().format(fmt).to_string();
                let dest_name = format!("{}{}", file_name, date_str);
                let mut dest_path = dest_base.join(&dest_name);

                if config.compress == Some(true) && config.delaycompress != Some(true) {
                    dest_path.set_extension(format!(
                        "{}{}",
                        dest_path.extension().unwrap_or_default().to_string_lossy(),
                        ".gz"
                    ));
                }

                if config.copytruncate == Some(true) {
                    fs::copy(path, &dest_path)?;
                    Self::truncate_file(path)?;
                } else {
                    fs::rename(path, &dest_path)?;
                    self.create_new_log(path, config)?;
                }

                if config.compress == Some(true) && config.delaycompress != Some(true) {
                    let uncompressed = dest_base.join(&dest_name);
                    compress_file(&uncompressed, &dest_path)?;
                    fs::remove_file(&uncompressed)?;
                }
            }
        } else {
            let max_rotate = config.rotate.unwrap_or(0);
            if max_rotate == 0 {
                self.rotate_without_backups(path, config)?;
            } else {
                let oldest_path = self.get_rotated_path(&dest_base, &file_name, max_rotate, config);
                if oldest_path.exists() {
                    fs::remove_file(&oldest_path)?;
                }

                for i in (1..max_rotate).rev() {
                    let src = self.get_rotated_path(&dest_base, &file_name, i, config);
                    let dest = self.get_rotated_path(&dest_base, &file_name, i + 1, config);
                    if src.exists() {
                        fs::rename(&src, &dest)?;
                    }
                }

                let dest_path_1 = self.get_rotated_path(&dest_base, &file_name, 1, config);

                if config.copytruncate == Some(true) {
                    let copy_dest =
                        if config.compress == Some(true) && config.delaycompress != Some(true) {
                            dest_base.join(format!("{}.1", file_name))
                        } else {
                            dest_path_1.clone()
                        };
                    fs::copy(path, &copy_dest)?;
                    Self::truncate_file(path)?;

                    if config.compress == Some(true) && config.delaycompress != Some(true) {
                        compress_file(&copy_dest, &dest_path_1)?;
                        fs::remove_file(&copy_dest)?;
                    }
                } else {
                    let rename_dest =
                        if config.compress == Some(true) && config.delaycompress != Some(true) {
                            dest_base.join(format!("{}.1", file_name))
                        } else {
                            dest_path_1.clone()
                        };

                    fs::rename(path, &rename_dest)?;
                    self.create_new_log(path, config)?;

                    if config.compress == Some(true) && config.delaycompress != Some(true) {
                        compress_file(&rename_dest, &dest_path_1)?;
                        fs::remove_file(&rename_dest)?;
                    }
                }

                if config.compress == Some(true) && config.delaycompress == Some(true) {
                    let second_path = self.get_rotated_path(&dest_base, &file_name, 2, config);
                    let second_path_uncompressed = dest_base.join(format!("{}.2", file_name));

                    if second_path_uncompressed.exists() {
                        compress_file(&second_path_uncompressed, &second_path)?;
                        fs::remove_file(&second_path_uncompressed)?;
                    }
                }
            }
        }

        self.state_db.update(&path_str);
        Ok(())
    }

    fn rotate_without_backups(&self, path: &Path, config: &LogConfig) -> Result<()> {
        if config.copytruncate == Some(true) {
            Self::truncate_file(path)
        } else {
            fs::remove_file(path)?;
            self.create_new_log(path, config)
        }
    }

    fn truncate_file(path: &Path) -> Result<()> {
        let file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)?;
        file.set_len(0)?;
        Ok(())
    }

    fn get_rotated_path(
        &self,
        dest_base: &Path,
        file_name: &str,
        index: u32,
        config: &LogConfig,
    ) -> PathBuf {
        let is_compressed = config.compress == Some(true);
        let is_delayed = config.delaycompress == Some(true);

        if is_compressed {
            if is_delayed && index == 1 {
                dest_base.join(format!("{}.{}", file_name, index))
            } else {
                dest_base.join(format!("{}.{}.gz", file_name, index))
            }
        } else {
            dest_base.join(format!("{}.{}", file_name, index))
        }
    }

    fn create_new_log(&self, path: &Path, config: &LogConfig) -> Result<()> {
        fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;

        if let Some(ref create_val) = config.create {
            let parts: Vec<&str> = create_val.split_whitespace().collect();
            if !parts.is_empty() {}
        }
        Ok(())
    }

    fn run_script(&self, script: &str, args: &str) -> Result<()> {
        if self.dry_run {
            println!("{} script: {}", "Would run".yellow().bold(), script);
            return Ok(());
        }

        if self.verbose {
            println!("Running script: {}", script);
        }

        let output = Command::new("sh")
            .arg("-c")
            .arg(format!("{} {}", script, args))
            .output();

        match output {
            Ok(out) => {
                if !out.status.success() {
                    eprintln!("Script failed with exit code: {:?}", out.status.code());
                    if !out.stderr.is_empty() {
                        eprintln!("Error output: {}", String::from_utf8_lossy(&out.stderr));
                    }
                }
            }
            Err(e) => {
                return Err(anyhow::Error::new(e).context("Failed to execute script"));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config_file;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("logrustate-{prefix}-{}-{seq}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_state_db(dir: &Path) -> StateDB {
        let state_path = dir.join("state");
        StateDB::load(state_path.to_str().unwrap()).unwrap()
    }

    #[test]
    fn test_rotate_zero_with_copytruncate_truncates_without_backup() {
        let dir = TempDir::new("zero-copytruncate");
        let log_path = dir.path().join("app.log");
        fs::write(&log_path, b"line 1\nline 2\n").unwrap();

        let mut state_db = test_state_db(dir.path());
        let mut rotator = Rotator::new(&mut state_db, false, false, true);
        let entry = LogEntry {
            paths: vec![log_path.to_string_lossy().to_string()],
            config: LogConfig {
                rotate: Some(0),
                copytruncate: Some(true),
                ..Default::default()
            },
        };

        rotator.process_entry(&entry).unwrap();

        assert_eq!(fs::read(&log_path).unwrap(), b"");
        assert!(!dir.path().join("app.log.1").exists());
        assert!(!dir.path().join("app.log.1.gz").exists());
        assert!(state_db.entries.contains_key(&*log_path.to_string_lossy()));
    }

    #[test]
    fn test_rotate_zero_without_copytruncate_replaces_without_backup() {
        let dir = TempDir::new("zero-no-copytruncate");
        let log_path = dir.path().join("app.log");
        fs::write(&log_path, b"old content\n").unwrap();

        let mut state_db = test_state_db(dir.path());
        let mut rotator = Rotator::new(&mut state_db, false, false, true);
        let entry = LogEntry {
            paths: vec![log_path.to_string_lossy().to_string()],
            config: LogConfig {
                rotate: Some(0),
                copytruncate: Some(false),
                ..Default::default()
            },
        };

        rotator.process_entry(&entry).unwrap();

        assert!(log_path.exists());
        assert_eq!(fs::read(&log_path).unwrap(), b"");
        assert!(!dir.path().join("app.log.1").exists());
    }

    #[test]
    fn test_rotate_zero_with_dateext_and_copytruncate_truncates_without_backup() {
        let dir = TempDir::new("zero-dateext");
        let log_path = dir.path().join("app.log");
        fs::write(&log_path, b"dated log data\n").unwrap();

        let mut state_db = test_state_db(dir.path());
        let mut rotator = Rotator::new(&mut state_db, false, false, true);
        let entry = LogEntry {
            paths: vec![log_path.to_string_lossy().to_string()],
            config: LogConfig {
                rotate: Some(0),
                dateext: Some(true),
                copytruncate: Some(true),
                ..Default::default()
            },
        };

        rotator.process_entry(&entry).unwrap();

        assert_eq!(fs::read(&log_path).unwrap(), b"");
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries, vec!["app.log"]);
    }

    #[test]
    fn test_rotate_one_with_copytruncate_keeps_one_backup() {
        let dir = TempDir::new("one-copytruncate");
        let log_path = dir.path().join("app.log");
        fs::write(&log_path, b"keep this line\n").unwrap();

        let mut state_db = test_state_db(dir.path());
        let mut rotator = Rotator::new(&mut state_db, false, false, true);
        let entry = LogEntry {
            paths: vec![log_path.to_string_lossy().to_string()],
            config: LogConfig {
                rotate: Some(1),
                copytruncate: Some(true),
                ..Default::default()
            },
        };

        rotator.process_entry(&entry).unwrap();

        assert_eq!(fs::read(&log_path).unwrap(), b"");
        assert_eq!(
            fs::read(dir.path().join("app.log.1")).unwrap(),
            b"keep this line\n"
        );
    }

    #[test]
    fn test_legacy_config_rotate_zero_copytruncate_force() {
        let dir = TempDir::new("legacy-zero");
        let log_path = dir.path().join("service.log");
        let conf_path = dir.path().join("service.conf");
        fs::write(&log_path, b"some log payload\n").unwrap();
        fs::write(
            &conf_path,
            format!(
                "{} {{\n    copytruncate\n    rotate 0\n}}\n",
                log_path.display()
            ),
        )
        .unwrap();

        let entries = parse_config_file(&conf_path).unwrap();
        let mut state_db = test_state_db(dir.path());
        let mut rotator = Rotator::new(&mut state_db, false, false, true);
        for entry in &entries {
            rotator.process_entry(entry).unwrap();
        }

        assert_eq!(fs::read(&log_path).unwrap(), b"");
        assert!(!dir.path().join("service.log.1").exists());
    }
}
