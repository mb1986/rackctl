//! Locating the configuration files.
//!
//! The configuration directory is `$XDG_CONFIG_HOME/rackctl`, or `~/.config/rackctl` when
//! `$XDG_CONFIG_HOME` is not set. It holds the rack file, `rack.kdl`, and the user's own
//! models in `catalog/`. The `-c` option and `$RACKCTL_CONFIG` name a different rack file;
//! the command-line parser handles them.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The environment variables that decide where the configuration is, read once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Locations {
    home: Option<PathBuf>,
    config_home: Option<PathBuf>,
}

impl Locations {
    /// Reads `$HOME` and `$XDG_CONFIG_HOME` from the environment of the process.
    pub fn from_env() -> Self {
        Self::new(env::var_os("HOME"), env::var_os("XDG_CONFIG_HOME"))
    }

    /// Creates locations from the values of `$HOME` and `$XDG_CONFIG_HOME`. Values that are
    /// empty or not absolute paths are ignored, as the XDG specification requires.
    pub fn new(home: Option<OsString>, config_home: Option<OsString>) -> Self {
        let absolute =
            |value: Option<OsString>| value.map(PathBuf::from).filter(|path| path.is_absolute());
        Self { home: absolute(home), config_home: absolute(config_home) }
    }

    /// Returns rackctl's configuration directory, or `None` when neither variable is set.
    pub fn config_dir(&self) -> Option<PathBuf> {
        let base =
            self.config_home.clone().or_else(|| Some(self.home.as_ref()?.join(".config")))?;
        Some(base.join("rackctl"))
    }

    /// Returns the default rack file, `rack.kdl` in the configuration directory.
    pub fn rack_file(&self) -> Option<PathBuf> {
        self.config_dir().map(|dir| dir.join("rack.kdl"))
    }

    /// Formats `path` for display, writing the home directory as `~`.
    pub fn display(&self, path: &Path) -> String {
        self.home.as_deref().and_then(|home| path.strip_prefix(home).ok()).map_or_else(
            || path.display().to_string(),
            |rest| Path::new("~").join(rest).display().to_string(),
        )
    }
}

/// Returns the directory of the user's own models for a rack file: `catalog/` next to it.
pub fn user_catalog(rack_file: &Path) -> PathBuf {
    rack_file.parent().unwrap_or_else(|| Path::new("")).join("catalog")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locations(home: Option<&str>, config_home: Option<&str>) -> Locations {
        Locations::new(home.map(OsString::from), config_home.map(OsString::from))
    }

    #[test]
    fn uses_the_xdg_config_home_first() {
        let locations = locations(Some("/home/u"), Some("/etc/xdg-u"));
        assert_eq!(locations.rack_file(), Some(PathBuf::from("/etc/xdg-u/rackctl/rack.kdl")));
    }

    #[test]
    fn falls_back_to_the_home_directory() {
        let expected = Some(PathBuf::from("/home/u/.config/rackctl/rack.kdl"));
        assert_eq!(locations(Some("/home/u"), None).rack_file(), expected);
        assert_eq!(locations(Some("/home/u"), Some("")).rack_file(), expected);
        assert_eq!(locations(Some("/home/u"), Some("relative")).rack_file(), expected);
    }

    #[test]
    fn finds_nothing_without_a_home() {
        assert_eq!(locations(None, None).rack_file(), None);
        assert_eq!(locations(Some("relative"), None).rack_file(), None);
    }

    #[test]
    fn shortens_the_home_directory() {
        let locations = locations(Some("/home/u"), None);
        assert_eq!(
            locations.display(Path::new("/home/u/.config/rackctl/rack.kdl")),
            "~/.config/rackctl/rack.kdl"
        );
        assert_eq!(locations.display(Path::new("/srv/rack.kdl")), "/srv/rack.kdl");
        assert_eq!(locations.display(Path::new("/home/user2/rack.kdl")), "/home/user2/rack.kdl");
    }

    #[test]
    fn finds_the_user_catalog_next_to_the_rack_file() {
        assert_eq!(
            user_catalog(Path::new("/srv/rack/rack.kdl")),
            PathBuf::from("/srv/rack/catalog")
        );
        assert_eq!(user_catalog(Path::new("rack.kdl")), PathBuf::from("catalog"));
    }
}
