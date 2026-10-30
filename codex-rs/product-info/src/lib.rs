pub const ASSISTANT_BRAND_ENV_VAR: &str = "ASSISTANT_BRAND";
#[deprecated(note = "use ASSISTANT_BRAND_ENV_VAR")]
pub const OPEN_INTERPRETER_BRAND_ENV_VAR: &str = ASSISTANT_BRAND_ENV_VAR;

/// Upstream Codex release whose client behavior is embedded in Assistant.
///
/// Assistant has an independent product version, so backend compatibility
/// checks must not interpret that product version as the embedded Codex version.
pub const ASSISTANT_CODEX_COMPATIBILITY_VERSION: &str = "0.144.5";
#[deprecated(note = "use ASSISTANT_CODEX_COMPATIBILITY_VERSION")]
pub const OPEN_INTERPRETER_CODEX_COMPATIBILITY_VERSION: &str = ASSISTANT_CODEX_COMPATIBILITY_VERSION;

const CODEX_RELEASE_NOTES_URL: &str = "https://github.com/openai/codex/releases/latest";
const ASSISTANT_RELEASE_NOTES_URL: &str =
    "https://github.com/hungpham10/assistant/releases/latest";
#[allow(dead_code)]
const OPEN_INTERPRETER_RELEASE_NOTES_URL: &str = ASSISTANT_RELEASE_NOTES_URL;
const CODEX_LATEST_RELEASE_URL: &str = "https://api.github.com/repos/openai/codex/releases/latest";
const ASSISTANT_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/hungpham10/assistant/releases/latest";
#[allow(dead_code)]
const OPEN_INTERPRETER_LATEST_RELEASE_URL: &str = ASSISTANT_LATEST_RELEASE_URL;
const CODEX_INSTALLER_URL: &str = "https://chatgpt.com/codex/install.sh";
const ASSISTANT_INSTALLER_URL: &str =
    "https://raw.githubusercontent.com/hungpham10/assistant/main/install/install.sh";
#[allow(dead_code)]
const OPEN_INTERPRETER_INSTALLER_URL: &str = ASSISTANT_INSTALLER_URL;
const CODEX_INSTALL_COMMAND: &str = "curl -fsSL https://chatgpt.com/codex/install.sh | sh";
const ASSISTANT_INSTALL_COMMAND: &str = "\
curl -fsSL https://raw.githubusercontent.com/hungpham10/assistant/main/install/install.sh | sh";
#[allow(dead_code)]
const OPEN_INTERPRETER_INSTALL_COMMAND: &str = ASSISTANT_INSTALL_COMMAND;
const CODEX_STANDALONE_UNIX_UPDATE_COMMAND: &str =
    "curl -fsSL https://chatgpt.com/codex/install.sh | CODEX_NON_INTERACTIVE=1 sh";
const ASSISTANT_STANDALONE_UNIX_UPDATE_COMMAND: &str = "\
curl -fsSL https://raw.githubusercontent.com/hungpham10/assistant/main/install/install.sh | ASSISTANT_NON_INTERACTIVE=1 sh";
#[allow(dead_code)]
const OPEN_INTERPRETER_STANDALONE_UNIX_UPDATE_COMMAND: &str =
    ASSISTANT_STANDALONE_UNIX_UPDATE_COMMAND;
const CODEX_STANDALONE_WINDOWS_UPDATE_COMMAND: &str =
    "$env:CODEX_NON_INTERACTIVE=1; irm https://chatgpt.com/codex/install.ps1 | iex";
const ASSISTANT_STANDALONE_WINDOWS_UPDATE_COMMAND: &str = "\
$env:ASSISTANT_NON_INTERACTIVE=1; \
irm https://raw.githubusercontent.com/hungpham10/assistant/main/install/install.ps1 | iex";
#[allow(dead_code)]
const OPEN_INTERPRETER_STANDALONE_WINDOWS_UPDATE_COMMAND: &str =
    ASSISTANT_STANDALONE_WINDOWS_UPDATE_COMMAND;
const CODEX_STANDALONE_UNIX_UPDATE_ARGS: &[&str] = &["-c", CODEX_STANDALONE_UNIX_UPDATE_COMMAND];
const ASSISTANT_STANDALONE_UNIX_UPDATE_ARGS: &[&str] =
    &["-c", ASSISTANT_STANDALONE_UNIX_UPDATE_COMMAND];
#[allow(dead_code)]
const OPEN_INTERPRETER_STANDALONE_UNIX_UPDATE_ARGS: &[&str] =
    ASSISTANT_STANDALONE_UNIX_UPDATE_ARGS;
const CODEX_STANDALONE_WINDOWS_UPDATE_ARGS: &[&str] = &[
    "-ExecutionPolicy",
    "Bypass",
    "-c",
    CODEX_STANDALONE_WINDOWS_UPDATE_COMMAND,
];
const ASSISTANT_STANDALONE_WINDOWS_UPDATE_ARGS: &[&str] = &[
    "-ExecutionPolicy",
    "Bypass",
    "-c",
    ASSISTANT_STANDALONE_WINDOWS_UPDATE_COMMAND,
];
#[allow(dead_code)]
const OPEN_INTERPRETER_STANDALONE_WINDOWS_UPDATE_ARGS: &[&str] =
    ASSISTANT_STANDALONE_WINDOWS_UPDATE_ARGS;

/// Product channel information used by branded package variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Product {
    Codex,
    #[deprecated(note = "use Assistant")]
    OpenInterpreter,
    Assistant,
}

impl Product {
    /// A binary shipped inside an Assistant package is Assistant unconditionally — identity must never depend on what the
    /// executable happens to be named or aliased to. The argv0 and env-var
    /// checks only exist for development builds run straight out of the
    /// cargo target directory.
    pub fn current() -> Self {
        if std::env::var_os(ASSISTANT_BRAND_ENV_VAR).is_some()
            || is_assistant_argv0()
            || is_assistant_install()
        {
            Self::Assistant
        } else {
            Self::Codex
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Product::Codex => "OpenAI Codex",
            Product::OpenInterpreter | Product::Assistant => "Assistant",
        }
    }

    pub fn command_name(self) -> &'static str {
        match self {
            Product::Codex => "codex",
            Product::OpenInterpreter | Product::Assistant => "assistant",
        }
    }

    /// Version advertised to Codex services for client compatibility checks.
    pub fn codex_compatibility_version(self) -> &'static str {
        match self {
            Product::Codex => env!("CARGO_PKG_VERSION"),
            Product::OpenInterpreter | Product::Assistant => ASSISTANT_CODEX_COMPATIBILITY_VERSION,
        }
    }

    pub fn release_notes_url(self) -> &'static str {
        match self {
            Product::Codex => CODEX_RELEASE_NOTES_URL,
            Product::OpenInterpreter | Product::Assistant => ASSISTANT_RELEASE_NOTES_URL,
        }
    }

    pub fn latest_release_url(self) -> &'static str {
        match self {
            Product::Codex => CODEX_LATEST_RELEASE_URL,
            Product::OpenInterpreter | Product::Assistant => ASSISTANT_LATEST_RELEASE_URL,
        }
    }

    pub fn installer_url(self) -> &'static str {
        match self {
            Product::Codex => CODEX_INSTALLER_URL,
            Product::OpenInterpreter | Product::Assistant => ASSISTANT_INSTALLER_URL,
        }
    }

    pub fn install_command(self) -> &'static str {
        match self {
            Product::Codex => CODEX_INSTALL_COMMAND,
            Product::OpenInterpreter | Product::Assistant => ASSISTANT_INSTALL_COMMAND,
        }
    }

    pub fn installer_env(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Product::Codex => &[],
            Product::OpenInterpreter | Product::Assistant => &[("ASSISTANT_NON_INTERACTIVE", "1")],
        }
    }

    pub fn standalone_unix_update_args(self) -> &'static [&'static str] {
        match self {
            Product::Codex => CODEX_STANDALONE_UNIX_UPDATE_ARGS,
            Product::OpenInterpreter | Product::Assistant => ASSISTANT_STANDALONE_UNIX_UPDATE_ARGS,
        }
    }

    pub fn standalone_windows_update_args(self) -> &'static [&'static str] {
        match self {
            Product::Codex => CODEX_STANDALONE_WINDOWS_UPDATE_ARGS,
            Product::OpenInterpreter | Product::Assistant => ASSISTANT_STANDALONE_WINDOWS_UPDATE_ARGS,
        }
    }
}

pub fn is_assistant_argv0() -> bool {
    std::env::args_os()
        .next()
        .and_then(|arg0| {
            std::path::Path::new(&arg0)
                .file_stem()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        })
        .is_some_and(|name| name.starts_with("assistant") || name == "interpreter" || name == "i")
}

#[deprecated(note = "use is_assistant_argv0")]
pub fn is_open_interpreter_argv0() -> bool {
    is_assistant_argv0()
}

/// Whether the running executable lives inside an installed Assistant
/// package (bin/<exe> next to the package's codex-package.json metadata).
fn is_assistant_install() -> bool {
    static IS_ASSISTANT_INSTALL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *IS_ASSISTANT_INSTALL.get_or_init(|| {
        std::env::current_exe()
            .ok()
            // current_exe can return the path the binary was invoked through
            // (for example a symlink elsewhere); resolve to the real location
            // so the package metadata next to the binary decides.
            .map(|exe| exe.canonicalize().unwrap_or(exe))
            .is_some_and(|exe| executable_is_in_assistant_package(&exe))
    })
}

#[deprecated(note = "use is_assistant_install")]
#[allow(dead_code)]
fn is_open_interpreter_install() -> bool {
    is_assistant_install()
}

fn executable_is_in_assistant_package(exe: &std::path::Path) -> bool {
    let Some(package_dir) = exe.parent().and_then(std::path::Path::parent) else {
        return false;
    };
    let Ok(metadata) = std::fs::read_to_string(package_dir.join("codex-package.json")) else {
        return false;
    };
    metadata
        .split('"')
        .collect::<Vec<_>>()
        .windows(3)
        .any(|window| {
            window[0] == "variant"
                && (window[2] == "assistant" || window[2] == "open-interpreter")
        })
}

#[deprecated(note = "use executable_is_in_assistant_package")]
fn executable_is_in_open_interpreter_package(exe: &std::path::Path) -> bool {
    executable_is_in_assistant_package(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_inside_assistant_package_is_assistant() {
        let package = tempfile::tempdir().expect("tempdir");
        let bin_dir = package.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("bin dir");
        let exe = bin_dir.join("codex");
        std::fs::write(&exe, b"").expect("exe");

        assert!(!executable_is_in_assistant_package(&exe));

        std::fs::write(
            package.path().join("codex-package.json"),
            br#"{ "layoutVersion": 1, "variant": "assistant", "entrypoint": "bin/assistant" }"#,
        )
        .expect("metadata");
        assert!(executable_is_in_assistant_package(&exe));

        std::fs::write(
            package.path().join("codex-package.json"),
            br#"{ "layoutVersion": 1, "variant": "open-interpreter", "entrypoint": "bin/interpreter" }"#,
        )
        .expect("metadata");
        assert!(executable_is_in_assistant_package(&exe));

        std::fs::write(
            package.path().join("codex-package.json"),
            br#"{ "layoutVersion": 1, "variant": "codex", "entrypoint": "bin/codex" }"#,
        )
        .expect("metadata");
        assert!(!executable_is_in_assistant_package(&exe));
    }

    #[test]
    fn binary_inside_open_interpreter_package_is_open_interpreter() {
        let package = tempfile::tempdir().expect("tempdir");
        let bin_dir = package.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("bin dir");
        let exe = bin_dir.join("codex");
        std::fs::write(&exe, b"").expect("exe");

        std::fs::write(
            package.path().join("codex-package.json"),
            br#"{ "layoutVersion": 1, "variant": "open-interpreter", "entrypoint": "bin/interpreter" }"#,
        )
        .expect("metadata");
        assert!(executable_is_in_assistant_package(&exe));
    }

    #[test]
    fn codex_product_uses_upstream_release_channel() {
        assert_eq!(
            Product::Codex.release_notes_url(),
            "https://github.com/openai/codex/releases/latest"
        );
        assert_eq!(
            Product::Codex.latest_release_url(),
            "https://api.github.com/repos/openai/codex/releases/latest"
        );
        assert_eq!(
            Product::Codex.installer_url(),
            "https://chatgpt.com/codex/install.sh"
        );
        assert_eq!(
            Product::Codex.install_command(),
            "curl -fsSL https://chatgpt.com/codex/install.sh | sh"
        );
        assert_eq!(Product::Codex.installer_env(), &[]);
        assert_eq!(
            Product::Codex.codex_compatibility_version(),
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn assistant_product_uses_assistant_release_channel() {
        assert_eq!(
            Product::Assistant.release_notes_url(),
            "https://github.com/hungpham10/assistant/releases/latest"
        );
        assert_eq!(
            Product::Assistant.latest_release_url(),
            "https://api.github.com/repos/hungpham10/assistant/releases/latest"
        );
        assert_eq!(
            Product::Assistant.installer_url(),
            "https://raw.githubusercontent.com/hungpham10/assistant/main/install/install.sh"
        );
        assert_eq!(
            Product::Assistant.codex_compatibility_version(),
            ASSISTANT_CODEX_COMPATIBILITY_VERSION
        );
        assert!(Product::Assistant
            .install_command()
            .contains("hungpham10/assistant"));
        assert_eq!(
            Product::Assistant.installer_env(),
            &[("ASSISTANT_NON_INTERACTIVE", "1")]
        );
    }

    #[test]
    fn open_interpreter_product_uses_assistant_release_channel() {
        #[allow(deprecated)]
        {
            assert_eq!(
                Product::OpenInterpreter.latest_release_url(),
                "https://api.github.com/repos/hungpham10/assistant/releases/latest"
            );
            assert_eq!(
                Product::OpenInterpreter.codex_compatibility_version(),
                ASSISTANT_CODEX_COMPATIBILITY_VERSION
            );
            assert_eq!(
                Product::OpenInterpreter.installer_env(),
                &[("ASSISTANT_NON_INTERACTIVE", "1")]
            );
        }
    }
}
