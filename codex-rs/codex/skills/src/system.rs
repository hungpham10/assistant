pub(crate) use crate::system_skills::install_system_skills;
pub(crate) use crate::system_skills::system_cache_root_dir;

use codex_path::absolute_path::AbsolutePathBuf;

pub(crate) fn uninstall_system_skills(codex_home: &AbsolutePathBuf) {
    let _ = std::fs::remove_dir_all(system_cache_root_dir(codex_home));
}
