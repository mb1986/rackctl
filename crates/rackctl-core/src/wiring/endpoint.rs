//! Endpoints as written in the wiring, such as `srv01:psu1` or `patch-32:b-f14`.

use std::fmt;

use miette::SourceSpan;

/// An endpoint as written: a device id and the name of one of its endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointRef {
    pub device: String,
    pub name: EndpointName,
    /// Location of the endpoint in the wiring file.
    pub span: SourceSpan,
}

/// The part of an endpoint after the device id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointName {
    /// A number in the device's main list, such as the `8` of `pdu:8`.
    Main(u16),
    /// A part, a port group or a patch side, with a number when one is written: `psu1`,
    /// `mgmt`, `XG1` or `b14`.
    Named { word: String, number: Option<u16> },
    /// A patch-panel port the path passes through, entering at `from`: `b-f14`.
    Through { from: PatchSide, number: u16 },
}

/// A side of a patch panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatchSide {
    Back,
    Front,
}

impl PatchSide {
    /// Reads a side written `b`, `back`, `f` or `front`.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "b" | "back" => Some(Self::Back),
            "f" | "front" => Some(Self::Front),
            _ => None,
        }
    }

    /// Returns the other side.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Back => Self::Front,
            Self::Front => Self::Back,
        }
    }

    /// Returns the side's name in messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Back => "back",
            Self::Front => "front",
        }
    }

    /// Returns the side's short name, as in `b14`.
    const fn short(self) -> char {
        match self {
            Self::Back => 'b',
            Self::Front => 'f',
        }
    }
}

impl fmt::Display for EndpointName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Main(number) => write!(f, "{number}"),
            Self::Named { word, number: Some(number) } => write!(f, "{word}{number}"),
            Self::Named { word, number: None } => write!(f, "{word}"),
            Self::Through { from, number } => {
                write!(f, "{}-{}{number}", from.short(), from.opposite().short())
            }
        }
    }
}

/// The ways an endpoint is written, for messages.
const FORMS: &str = "write `device:endpoint`, such as `pdu:8`, `srv01:psu1`, `sg350x:XG1`, \
                     `srv01:mgmt` or `patch-32:b14`";

/// Splits `text` into a device id and an endpoint name.
///
/// # Errors
///
/// Returns why `text` is not an endpoint.
pub fn parse_endpoint(text: &str) -> Result<(&str, EndpointName), String> {
    let (device, name) = match text.split_once(':') {
        Some((device, name)) if !device.is_empty() && !name.is_empty() => (device, name),
        _ => return Err(format!("`{text}` is not an endpoint: {FORMS}")),
    };
    let (words, digits) =
        name.split_at(name.find(|c: char| c.is_ascii_digit()).unwrap_or(name.len()));
    let number = match digits {
        "" => None,
        digits if digits.chars().all(|c| c.is_ascii_digit()) => Some(
            digits
                .parse::<u16>()
                .map_err(|_| format!("endpoint numbers go up to 65535, found {digits}"))?,
        ),
        _ => return Err(format!("`{name}` is not an endpoint name: {FORMS}")),
    };
    let name = match (words.split_once('-'), number) {
        (None, Some(number)) if words.is_empty() => EndpointName::Main(number),
        (None, number) if words.chars().all(|c| c.is_ascii_alphabetic()) => {
            EndpointName::Named { word: words.to_owned(), number }
        }
        (Some((from, to)), number) => through(from, to, number, name)?,
        _ => return Err(format!("`{name}` is not an endpoint name: {FORMS}")),
    };
    Ok((device, name))
}

/// Reads a patch port passed through from side `from` to side `to`, written `name`.
fn through(from: &str, to: &str, number: Option<u16>, name: &str) -> Result<EndpointName, String> {
    let side = |word| {
        PatchSide::parse(word)
            .ok_or_else(|| format!("a patch side is `b`, `back`, `f` or `front`, found `{word}`"))
    };
    let (from, to) = (side(from)?, side(to)?);
    if from == to {
        return Err(format!(
            "a path passes through a patch port from one side to the other, such as `b-f14`, \
             found `{name}`"
        ));
    }
    let number =
        number.ok_or_else(|| format!("`{name}` needs the port's number, such as `b-f14`"))?;
    Ok(EndpointName::Through { from, number })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(word: &str, number: Option<u16>) -> EndpointName {
        EndpointName::Named { word: word.to_owned(), number }
    }

    #[test]
    fn reads_every_form_of_endpoint() {
        let through = |from, number| EndpointName::Through { from, number };
        for (text, device, name) in [
            ("pdu:8", "pdu", EndpointName::Main(8)),
            ("router:0", "router", EndpointName::Main(0)),
            ("srv01:psu1", "srv01", named("psu", Some(1))),
            ("srv01:mgmt", "srv01", named("mgmt", None)),
            ("sg350x:XG1", "sg350x", named("XG", Some(1))),
            ("patch-32:b14", "patch-32", named("b", Some(14))),
            ("patch-32:front14", "patch-32", named("front", Some(14))),
            ("patch-32:b-f14", "patch-32", through(PatchSide::Back, 14)),
            ("patch-32:front-back3", "patch-32", through(PatchSide::Front, 3)),
        ] {
            assert_eq!(parse_endpoint(text), Ok((device, name)), "{text}");
        }
    }

    #[test]
    fn explains_what_is_not_an_endpoint() {
        let error = |text| parse_endpoint(text).expect_err(text);
        let forms = format!("is not an endpoint: {FORMS}");
        assert_eq!(error("srv01"), format!("`srv01` {forms}"));
        assert_eq!(error(":psu1"), format!("`:psu1` {forms}"));
        assert_eq!(error("srv01:"), format!("`srv01:` {forms}"));
        assert_eq!(error("srv01:psu1a"), format!("`psu1a` is not an endpoint name: {FORMS}"));
        assert_eq!(error("srv01:nic_1"), format!("`nic_1` is not an endpoint name: {FORMS}"));
        assert_eq!(error("srv01:psu:1"), format!("`psu:1` is not an endpoint name: {FORMS}"));
        assert_eq!(error("pdu:65536"), "endpoint numbers go up to 65535, found 65536");
        assert_eq!(error("p:x-f1"), "a patch side is `b`, `back`, `f` or `front`, found `x`");
        assert_eq!(
            error("p:b-b1"),
            "a path passes through a patch port from one side to the other, such as `b-f14`, \
             found `b-b1`"
        );
        assert_eq!(error("p:b-f"), "`b-f` needs the port's number, such as `b-f14`");
    }
}
