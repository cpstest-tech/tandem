use super::{Job, Request, Stage};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: &Path) -> Result<Self, String> {
        let root = root.to_path_buf();
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        if std::fs::symlink_metadata(&root)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("AI source storage must not be a symbolic link".into());
        }
        Ok(Self { root })
    }

    pub fn directory(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty()
            || id.len() > 100
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("Invalid AI sequence identifier".into());
        }
        let path = self.root.join(id);
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Sequence directories must not be symbolic links".into());
        }
        Ok(path)
    }

    pub fn create(&self, request: Request) -> Result<Job, String> {
        request.composition.validate()?;
        request.limits.validate()?;
        if request.prompt.trim().is_empty()
            || request.prompt.len() > 64_000
            || request.context.len() > 512_000
        {
            return Err("Provide a prompt and a context smaller than 512 KB".into());
        }
        if let Some(parent) = &request.parent {
            let previous = self.load(parent)?;
            if previous.stage != Stage::Completed {
                return Err("Only completed sequences can be revised".into());
            }
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = format!(
            "{}-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        std::fs::create_dir(self.directory(&id)?).map_err(|e| e.to_string())?;
        let job = Job {
            id,
            request,
            stage: Stage::Queued,
            error: None,
            attempt: 0,
            source: None,
            usage: None,
            usage_history: Vec::new(),
            imported: false,
            session: None,
            events: Vec::new(),
            attachments: Vec::new(),
            improvements: Vec::new(),
            slots: Vec::new(),
        };
        if let Err(error) = self.save(&job) {
            let _ = std::fs::remove_dir(self.directory(&job.id)?);
            return Err(error);
        }
        Ok(job)
    }

    pub fn load(&self, id: &str) -> Result<Job, String> {
        let path = self.directory(id)?.join("job.json");
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
            return Err("Sequence manifest is not a regular file smaller than 4 MB".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(|e| e.to_string())?
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("Sequence manifest exceeds 4 MB".into());
        }
        let job: Job = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if job.id != id {
            return Err("Sequence manifest identity mismatch".into());
        }
        Ok(job)
    }

    pub fn save(&self, job: &Job) -> Result<(), String> {
        let directory = self.directory(&job.id)?;
        let path = directory.join("job.json");
        let temporary = directory.join("job.pending");
        let bytes = serde_json::to_vec_pretty(job).map_err(|e| e.to_string())?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
        let result = (|| {
            std::io::Write::write_all(&mut file, &bytes).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            std::fs::rename(&temporary, &path).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    pub fn exclusive(&self) -> Result<std::fs::File, String> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let file = options
            .open(self.root.join(".queue.lock"))
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err("Another editor instance is managing this project's AI jobs".into());
            }
        }
        Ok(file)
    }

    #[cfg(test)]
    pub fn recover(&self) -> Result<Vec<Job>, String> {
        self.recover_report().map(|report| report.0)
    }

    pub fn recover_report(&self) -> Result<(Vec<Job>, Vec<String>), String> {
        let mut jobs = Vec::new();
        let mut warnings = Vec::new();
        for entry in std::fs::read_dir(&self.root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                continue;
            }
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if id == ".analyses" {
                continue;
            }
            let result = (|| {
                let directory = self.directory(&id)?;
                let pending = directory.join("job.pending");
                if std::fs::symlink_metadata(&pending).is_ok() {
                    std::fs::remove_file(pending).map_err(|e| e.to_string())?;
                }
                let mut job = self.load(&id)?;
                if !job.stage.terminal() {
                    if std::fs::symlink_metadata(directory.join("cancel.requested"))
                        .is_ok_and(|metadata| metadata.is_file())
                    {
                        job.stage = Stage::Cancelled;
                        job.error = Some("Cancelled before application shutdown".into());
                    } else {
                        job.stage = Stage::Failed;
                        job.error=Some("Generation interrupted by application shutdown; retry to create a new job".into());
                    }
                    super::render::discard_partial(&directory);
                    self.save(&job)?;
                }
                Ok::<_, String>(job)
            })();
            match result {
                Ok(job) => jobs.push(job),
                Err(error) => warnings.push(format!(
                    "Sequence {id}: {error}. Existing files were preserved."
                )),
            }
        }
        jobs.sort_by(|a, b| a.id.cmp(&b.id));
        Ok((jobs, warnings))
    }
}
