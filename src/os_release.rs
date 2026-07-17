use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Distro families the tool knows how to prepare. Detection order matches the
/// original scripts: Debian first, then Arch, then Fedora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distro {
    Debian,
    Arch,
    Fedora,
    Unknown,
}

impl Distro {
    pub fn detect(os_release: &Path) -> Distro {
        fs::read_to_string(os_release).map(|s| Self::from_os_release(&s)).unwrap_or(Distro::Unknown)
    }

    /// Matches on `ID` and every token of `ID_LIKE`, so derivatives such as Mint
    /// (`ID_LIKE="ubuntu debian"`), Manjaro or Nobara are recognised too.
    pub fn from_os_release(content: &str) -> Distro {
        let vars = parse(content);
        let ids: Vec<&str> =
            vars.get("ID").into_iter().chain(vars.get("ID_LIKE")).flat_map(|v| v.split_whitespace()).collect();
        let has = |name: &str| ids.contains(&name);
        if has("debian") || has("ubuntu") {
            Distro::Debian
        } else if has("arch") {
            Distro::Arch
        } else if has("fedora") {
            Distro::Fedora
        } else {
            Distro::Unknown
        }
    }
}

/// Parses `KEY=value` lines per os-release(5): comments, blank lines and single or
/// double quoting (with backslash escapes inside double quotes).
pub fn parse(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), unquote(v.trim())))
        .collect()
}

fn unquote(v: &str) -> String {
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return v[1..v.len() - 1].to_string();
    }
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let mut out = String::new();
        let mut chars = v[1..v.len() - 1].chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                out.extend(chars.next());
            } else {
                out.push(c);
            }
        }
        return out;
    }
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_families() {
        let cases = [
            ("ID=arch\n", Distro::Arch),
            ("ID=ubuntu\nID_LIKE=debian\n", Distro::Debian),
            ("ID=debian\n", Distro::Debian),
            ("ID=fedora\nVERSION_ID=41\n", Distro::Fedora),
            ("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n", Distro::Debian),
            ("ID=manjaro\nID_LIKE=arch\n", Distro::Arch),
            ("ID=endeavouros\nID_LIKE=\"arch\"\n", Distro::Arch),
            ("ID=nobara\nID_LIKE=\"rhel centos fedora\"\n", Distro::Fedora),
            ("ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n", Distro::Unknown),
            ("", Distro::Unknown),
        ];
        for (content, expected) in cases {
            assert_eq!(Distro::from_os_release(content), expected, "{content:?}");
        }
    }

    #[test]
    fn parses_quotes_and_comments() {
        let vars = parse("# comment\nNAME=\"Arch \\\"Linux\\\"\"\nID='arch'\n\nBUILD_ID=rolling\n");
        assert_eq!(vars["NAME"], "Arch \"Linux\"");
        assert_eq!(vars["ID"], "arch");
        assert_eq!(vars["BUILD_ID"], "rolling");
    }

    #[test]
    fn missing_file_is_unknown() {
        assert_eq!(Distro::detect(Path::new("/nonexistent/os-release")), Distro::Unknown);
    }
}
