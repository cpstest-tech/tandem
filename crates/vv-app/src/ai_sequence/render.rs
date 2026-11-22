use super::{Job, Runtime};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const DRIVER: &str = r#"
const path = require('node:path');
const [browserExecutable, mode, runtime, work] = process.argv.slice(2);
const {bundle} = require(path.join(runtime, 'node_modules/@remotion/bundler'));
const {selectComposition, renderMedia} = require(path.join(runtime, 'node_modules/@remotion/renderer'));
(async () => {
  const serveUrl = await bundle({entryPoint:path.join(work, 'index.tsx'), publicDir: path.join(work, 'public')});
  const composition = await selectComposition({serveUrl, id:'Sequence', browserExecutable, logLevel:'verbose'});
  if (mode === 'validate') return;
  const transparent = mode === 'transparent';
  await renderMedia({serveUrl, composition, browserExecutable,
    codec: transparent ? 'prores' : 'h264',
    ...(transparent ? {proResProfile:'4444', pixelFormat:'yuva444p10le', imageFormat:'png'} : {}),
    outputLocation:path.join(work, transparent ? 'result.mov' : 'result.mp4'), concurrency:1});
})().catch(e => {
  const message=e.stack || String(e);
  const environment=/ENOSPC|EROFS|EACCES|ENOMEM|Failed to launch the browser|browser executable|spawn.*ENOENT/i.test(message);
  console.error((environment ? 'TANDEM_RENDER_ENVIRONMENT_ERROR: ' : 'TANDEM_COMPOSITION_ERROR: ')+message);
  process.exit(1);
});
"#;

pub fn prepare(job: &Job, directory: &Path) -> Result<(), String> {
    discard_partial(directory);
    let config = &job.request.composition;
    config.validate()?;
    std::fs::write(
        directory.join("Scene.tsx"),
        job.source.as_deref().ok_or("Missing source")?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(directory.join("index.tsx"), super::material::entry(job)?)
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(directory.join("public")).map_err(|e| e.to_string())?;
    std::fs::write(directory.join("driver.cjs"), DRIVER).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(directory.join("node_modules")).map_err(|e| e.to_string())
}

pub fn run(
    runtime: &Runtime,
    job: &Job,
    directory: &Path,
    validate: bool,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let root = runtime
        .directory
        .canonicalize()
        .map_err(|e| format!("Configure a local Remotion runtime: {e}"))?;
    let node = runtime
        .node
        .canonicalize()
        .map_err(|e| format!("Configure the Node executable: {e}"))?;
    let browser = runtime
        .browser
        .canonicalize()
        .map_err(|e| format!("Configure the Chromium executable: {e}"))?;
    let relative_browser = browser
        .strip_prefix(&root)
        .map_err(|_| "Chromium must be installed inside the Remotion runtime directory")?;
    if !root.join("node_modules/@remotion/renderer").is_dir()
        || !root.join("node_modules/@remotion/bundler").is_dir()
    {
        return Err(
            "Install @remotion/renderer and @remotion/bundler in the configured runtime".into(),
        );
    }
    let mode = if validate {
        "validate"
    } else if job.request.composition.transparent {
        "transparent"
    } else {
        "opaque"
    };
    let work = directory.canonicalize().map_err(|e| e.to_string())?;
    let mut command = if cfg!(target_os = "linux") {
        // Video frames come from a local server Node reaches by name: without
        // these the lookup goes to DNS, which the isolated network lacks.
        std::fs::write(
            work.join("sandbox-hosts"),
            "127.0.0.1 localhost\n::1 localhost\n",
        )
        .and_then(|_| std::fs::write(work.join("sandbox-nsswitch.conf"), "hosts: files\n"))
        .map_err(|e| e.to_string())?;
        sandboxed(&root, &node, &work, relative_browser, mode)
    } else {
        check_imports(job.source.as_deref().unwrap_or_default())?;
        unsandboxed(&root, &node, &work, &browser, mode)
    };
    super::process::run(
        &mut command,
        cancel,
        Duration::from_secs(if validate {
            job.request.limits.render_timeout_secs.min(120)
        } else {
            job.request.limits.render_timeout_secs
        }),
    )?;
    let output = directory.join(if job.request.composition.transparent {
        "result.mov"
    } else {
        "result.mp4"
    });
    if !validate && !output.is_file() {
        return Err("Renderer did not produce a media file".into());
    }
    Ok(output)
}

fn sandboxed(
    root: &Path,
    node: &Path,
    work: &Path,
    relative_browser: &Path,
    mode: &str,
) -> Command {
    let mut command = Command::new("bwrap");
    command.env_clear().args([
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
    ]);
    for path in ["/usr", "/lib", "/lib64", "/etc/fonts", "/etc/ld.so.cache"] {
        if Path::new(path).exists() {
            command.args(["--ro-bind", path, path]);
        }
    }
    command
        .arg("--ro-bind")
        .arg(root)
        .arg("/runtime")
        .arg("--ro-bind")
        .arg(node)
        .arg("/node")
        .arg("--bind")
        .arg(work)
        .arg("/work")
        .arg("--ro-bind")
        .arg(root.join("node_modules"))
        .arg("/work/node_modules")
        .arg("--ro-bind")
        .arg(work.join("sandbox-hosts"))
        .arg("/etc/hosts")
        .arg("--ro-bind")
        .arg(work.join("sandbox-nsswitch.conf"))
        .arg("/etc/nsswitch.conf")
        .args([
            "--chdir",
            "/work",
            "--setenv",
            "HOME",
            "/tmp",
            "--setenv",
            "PATH",
            "/usr/bin:/bin",
            "--",
            "/node",
            "--max-old-space-size=1024",
            "/work/driver.cjs",
        ])
        .arg(Path::new("/runtime").join(relative_browser))
        .args([mode, "/runtime", "/work"]);
    command
}

// No bubblewrap outside Linux: the scene runs with the user's rights, so at
// least webpack must not be able to bundle arbitrary local files into it.
fn unsandboxed(root: &Path, node: &Path, work: &Path, browser: &Path, mode: &str) -> Command {
    let home = work.join("home");
    let _ = std::fs::create_dir_all(&home);
    let mut command = Command::new(node);
    command.env_clear();
    // Windows needs these to start Chromium and resolve network names.
    for name in ["SYSTEMROOT", "WINDIR", "SYSTEMDRIVE", "PATH", "PATHEXT"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    for name in ["HOME", "USERPROFILE", "TMP", "TEMP", "TMPDIR"] {
        command.env(name, &home);
    }
    command
        .current_dir(work)
        .arg("--max-old-space-size=1024")
        .arg(work.join("driver.cjs"))
        .arg(browser)
        .arg(mode)
        .arg(root)
        .arg(work);
    command
}

/// The scene may import only React and Remotion, statically.
pub(crate) fn check_imports(source: &str) -> Result<(), String> {
    let allowed = |module: &str| {
        matches!(module, "react" | "remotion")
            || module.starts_with("react/")
            || module.starts_with("@remotion/")
    };
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !is_ident(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_ident(bytes[i]) {
            i += 1;
        }
        let word = &source[start..i];
        let after = source[i..].trim_start();
        match word {
            "require" | "import" if after.starts_with('(') => {
                return Err(composition_error(&format!("{word}() is not allowed")));
            }
            "import" | "from" => {
                if let Some(module) = quoted(after).filter(|module| !allowed(module)) {
                    return Err(composition_error(&format!(
                        "import of '{module}' is not allowed: use only react and remotion"
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn quoted(text: &str) -> Option<&str> {
    let quote = text
        .chars()
        .next()
        .filter(|c| matches!(c, '\'' | '"' | '`'))?;
    text[1..].split(quote).next()
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

fn composition_error(message: &str) -> String {
    format!("TANDEM_COMPOSITION_ERROR: {message}")
}

pub fn preflight(runtime: &Runtime, cancel: &AtomicBool) -> Result<(), String> {
    let root = runtime
        .directory
        .canonicalize()
        .map_err(|_| "Configure the local Remotion runtime before generating")?;
    if !runtime.node.is_file()
        || !runtime.browser.is_file()
        || !runtime
            .browser
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&root)
        || !root.join("node_modules/@remotion/renderer").is_dir()
        || !root.join("node_modules/@remotion/bundler").is_dir()
    {
        return Err("Configure Node, Chromium inside the runtime, and the installed Remotion packages before generating".into());
    }
    if !cfg!(target_os = "linux") {
        return Ok(());
    }
    super::process::run(
        Command::new("bwrap").args([
            "--unshare-all",
            "--die-with-parent",
            "--ro-bind",
            "/",
            "/",
            "--",
            "/usr/bin/true",
        ]),
        cancel,
        Duration::from_secs(10),
    )
    .map_err(|e| format!("Local rendering isolation is unavailable: {e}"))?;
    Ok(())
}

pub fn repairable(error: &str) -> bool {
    error.contains("TANDEM_COMPOSITION_ERROR:")
}

pub fn discard_partial(directory: &Path) {
    for name in ["result.mp4", "result.mov"] {
        let path = directory.join(name);
        if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
#[path = "../tests/ai_sequence/render.rs"]
mod tests;
