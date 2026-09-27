//! Locating the configuration files.
//!
//! The configuration directory is `$XDG_CONFIG_HOME/rackctl`, or `~/.config/rackctl` when
//! `$XDG_CONFIG_HOME` is not set. It holds the rack file, `rack.kdl`, and the user's own
//! models in `catalog/`. `$RACKCTL_CONFIG` names a different rack file, and the `-c` option,
//! handled by the command-line parser, overrides both.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The environment variables that decide where the configuration is, read once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Locations {
    home: Option<PathBuf>,
    config_home: Option<PathBuf>,
    rack_config: Option<PathBuf>,
}

impl Locations {
    /// Reads `$HOME`, `$XDG_CONFIG_HOME` and `$RACKCTL_CONFIG`.
    pub fn from_env() -> Self {
        Self::new(
            env::var_os("HOME"),
            env::var_os("XDG_CONFIG_HOME"),
            env::var_os("RACKCTL_CONFIG"),
        )
    }

    /// Creates locations from the values of the three variables. Empty values are ignored,
    /// and so are relative directories, as the XDG specification requires.
    pub fn new(
        home: Option<OsString>,
        config_home: Option<OsString>,
        rack_config: Option<OsString>,
    ) -> Self {
        let absolute =
            |value: Option<OsString>| value.map(PathBuf::from).filter(|path| path.is_absolute());
        Self {
            home: absolute(home),
            config_home: absolute(config_home),
            rack_config: rack_config.filter(|value| !value.is_empty()).map(PathBuf::from),
        }
    }

    /// Returns rackctl's configuration directory, or `None` when neither variable is set.
    pub fn config_dir(&self) -> Option<PathBuf> {
        let base =
            self.config_home.clone().or_else(|| Some(self.home.as_ref()?.join(".config")))?;
        Some(base.join("rackctl"))
    }

    /// Returns the rack file to use when `-c` is not given: `$RACKCTL_CONFIG`, or
    /// `rack.kdl` in the configuration directory.
    pub fn rack_file(&self) -> Option<PathBuf> {
        self.rack_config.clone().or_else(|| Some(self.config_dir()?.join("rack.kdl")))
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
        Locations::new(home.map(OsString::from), config_home.map(OsString::from), None)
    }

    #[test]
    fn uses_rackctl_config_when_it_is_not_empty() {
        let with = |value: &str| {
            Locations::new(Some("/home/u".into()), None, Some(value.into())).rack_file()
        };
        assert_eq!(with("/srv/rack.kdl"), Some(PathBuf::from("/srv/rack.kdl")));
        assert_eq!(with("rack.kdl"), Some(PathBuf::from("rack.kdl")));
        assert_eq!(with(""), Some(PathBuf::from("/home/u/.config/rackctl/rack.kdl")));
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
