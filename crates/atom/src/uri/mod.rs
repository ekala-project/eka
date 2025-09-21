//! # Atom URI Format
//!
//! An Atom URI of the form:
//! ```text
//! [scheme://][alias:][url-fragment::]atom-id[@version]
//! ```
//!
//! An `alias` is a user configurable URL shortener that must at least contain an FQDN or host,
//! and as much of the url path as desirable. Aliases can be specified in the eka configuration
//! file for the CLI program. See the Atom configuration crate for further detail.
//!
//! ## Examples
//! * `gh:owner/repo::my-atom` where `hub` is `github.com`
//! * `work:repo::my-atom` where `work` is `github.com/my-work-org`
//! * `repo::my-atom@^1` where `repo` is `example.com/some/repo`
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::ops::Deref;
use std::str::FromStr;

use gix_url::Url;
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::id::Id;
use crate::id::Error;

#[derive(Debug)]
struct Aliases(&'static HashMap<&'static str, &'static str>);

/// Represents the parsed components of an Atom URI.
///
/// It is typically created through the `FromStr` implementation, not constructed directly.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Uri {
    /// The URL to the repository containing the Atom.
    url: Option<Url>,
    /// The Atom's ID.
    id: Id,
    /// The requested Atom version.
    version: Option<VersionReq>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(Serialize, Deserialize))]
struct Ref<'a> {
    #[cfg_attr(test, serde(borrow))]
    url: UrlRef<'a>,
    atom: AtomRef<'a>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[cfg_attr(test, derive(Serialize, Deserialize))]
struct UrlRef<'a> {
    /// The URI scheme (e.g., "https", "ssh"), if present.
    scheme: Option<&'a str>,
    /// The username.
    user: Option<&'a str>,
    /// The password.
    pass: Option<&'a str>,
    /// A URL fragment which may contain an alias to be later expanded
    frag: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(Serialize, Deserialize))]
struct AtomRef<'a> {
    /// The path to the specific Atom within the repository.
    id: Option<&'a str>,
    /// The version of the Atom, if specified.
    version: Option<&'a str>,
}

use nom::branch::alt;
use nom::bytes::complete::{tag, take_until};
use nom::character::complete::digit1;
use nom::combinator::{all_consuming, map, not, opt, peek, rest, verify};
use nom::sequence::{separated_pair, tuple};
use nom::{IResult, ParseTo};

fn parse(input: &str) -> Ref<'_> {
    let (rest, url) = match url(input) {
        Ok(s) => s,
        Err(_) => (input, None),
    };

    let url = url.map(UrlRef::from).unwrap_or_default();

    let atom = AtomRef::from(rest);

    tracing::trace!(
        url.scheme,
        url.user,
        url.pass = url.pass.map(|_| "<redacted>"),
        url.frag,
        atom.id,
        atom.version,
        "{}",
        input
    );

    Ref { url, atom }
}

fn parse_alias(input: &str) -> (&str, Option<&str>) {
    opt(verify(
        map(
            alt((
                tuple((
                    take_until::<_, _, ()>(":"),
                    tag(":"),
                    // not a port
                    peek(not(digit1)),
                )),
                map(rest, |a| (a, "", ())),
            )),
            |(a, ..)| a,
        ),
        // not an scp url
        |a| {
            !(a as &str)
                .chars()
                .any(|c| c == ':' || c == '/' || c == '.')
        },
    ))(input)
    .map(empty_none)
    .unwrap_or((input, None))
}

fn parse_host(input: &str) -> IResult<&str, (&str, &str)> {
    alt((first_path, ssh_host, map(rest, |a| (a, ""))))(input)
}

fn parse_port(input: &str) -> IResult<&str, Option<(&str, &str)>> {
    opt(all_consuming(separated_pair(
        take_until(":"),
        tag(":"),
        digit1,
    )))(input)
}

fn ssh_host(input: &str) -> IResult<&str, (&str, &str)> {
    let (rest, (host, colon)) = tuple((take_until(":"), tag(":")))(input)?;

    let (rest, port) = opt(tuple((peek(digit1), take_until(":"), tag(":"))))(rest)?;

    match port {
        Some((_, port_str, second_colon)) => {
            let full_host = &input[..(host.len() + colon.len() + port_str.len())];
            Ok((rest, (full_host, second_colon)))
        },
        None => Ok((rest, (host, colon))),
    }
}

fn first_path(input: &str) -> IResult<&str, (&str, &str)> {
    tuple((
        verify(take_until("/"), |h: &str| {
            !h.contains(':') || parse_port(h).ok().and_then(|(_, p)| p).is_some()
        }),
        tag("/"),
    ))(input)
}

type UrlPrefix<'a> = (Option<&'a str>, Option<&'a str>, Option<&'a str>);

fn parse_url(url: &str) -> IResult<&str, UrlPrefix<'_>> {
    let (rest, (scheme, user_pass)) = tuple((scheme, split_at))(url)?;

    let (user, pass) = match user_pass {
        Some(s) => match split_colon(s) {
            Ok((p, Some(u))) => (Some(u), Some(p)),
            Ok((u, None)) => (Some(u), None),
            _ => (Some(s), None),
        },
        None => (None, None),
    };

    Ok((rest, (scheme, user, pass)))
}

fn not_empty(input: &str) -> Option<&str> {
    if input.is_empty() { None } else { Some(input) }
}

fn empty_none<'a>((rest, opt): (&'a str, Option<&'a str>)) -> (&'a str, Option<&'a str>) {
    (rest, opt.and_then(not_empty))
}

fn opt_split<'a>(input: &'a str, delim: &str) -> IResult<&'a str, Option<&'a str>> {
    opt(map(tuple((take_until(delim), tag(delim))), |(url, _)| url))(input).map(empty_none)
}

fn url(input: &str) -> IResult<&str, Option<&str>> {
    opt_split(input, "::")
}

fn scheme(input: &str) -> IResult<&str, Option<&str>> {
    opt_split(input, "://")
}

fn split_at(input: &str) -> IResult<&str, Option<&str>> {
    opt_split(input, "@")
}

fn split_colon(input: &str) -> IResult<&str, Option<&str>> {
    opt_split(input, ":")
}

impl<'a> From<&'a str> for Ref<'a> {
    /// Parses a string slice into a `Ref`.
    ///
    /// This is the primary way to create a `Ref` instance.
    ///
    /// # Arguments
    ///
    /// * `input` - A string slice containing the URI to parse.
    ///
    /// # Returns
    ///
    /// A `Ref` instance representing the parsed URI.
    fn from(input: &'a str) -> Self {
        parse(input)
    }
}

/// A error encountered when constructing the concrete types from an Atom URI
#[derive(Error, Debug)]
pub enum UriError {
    /// An alias uses the same validation logic as the Unicode Atom identifier.
    #[error(transparent)]
    AliasValidation(#[from] Error),
    /// The version requested is not valid.
    #[error(transparent)]
    InvalidVersionReq(#[from] semver::Error),
    /// The Url did not parse correctly.
    #[error(transparent)]
    UrlParse(#[from] gix_url::parse::Error),
    /// There is no alias in the configuration matching the one given in the URI.
    #[error("The passed alias does not exist: {0}")]
    NoAlias(String),
    /// The Url is invalid
    #[error("Parsing URL failed")]
    NoUrl,
    #[error("Missing the required Atom ID in URI")]
    /// The Atom identifier is missing, but required
    NoAtom,
}

use std::borrow::Cow;
impl Aliases {
    fn get_alias(&self, s: &str) -> Result<&str, UriError> {
        self.get(s)
            .map_or_else(|| Err(UriError::NoAlias(s.into())), |s| Ok(*s))
    }

    fn resolve_alias(&'static self, s: &str) -> Result<Cow<'static, str>, UriError> {
        let res = self.get_alias(s)?;

        // allow one level of indirection in alises, e.g. `org = gh:my-org`
        let res = match res.split_once(':') {
            Some((s, rest)) => {
                let res = self.get_alias(s)?;
                Cow::Owned(format!("{res}/{rest}"))
            },
            None => Cow::Borrowed(res),
        };

        Ok(res)
    }
}

impl Deref for Aliases {
    type Target = HashMap<&'static str, &'static str>;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'a> From<&'a str> for UrlRef<'a> {
    fn from(s: &'a str) -> Self {
        let (scheme, user, pass, frag) = match parse_url(s) {
            Ok((frag, (scheme, user, pass))) => (scheme, user, pass, not_empty(frag)),
            _ => (None, None, None, None),
        };

        Self {
            scheme,
            user,
            pass,
            frag,
        }
    }
}

impl<'a> From<&'a str> for AtomRef<'a> {
    fn from(s: &'a str) -> Self {
        let (id, version) = match split_at(s) {
            Ok((rest, Some(atom))) => (Some(atom), not_empty(rest)),
            Ok((rest, None)) => (not_empty(rest), None),
            _ => (None, None),
        };

        AtomRef { id, version }
    }
}

use std::sync::LazyLock;
static ALIASES: LazyLock<Aliases> = LazyLock::new(|| Aliases(config::CONFIG.aliases()));

impl<'a> UrlRef<'a> {
    fn render_alias(&self) -> Option<(&str, Option<Cow<'static, str>>)> {
        let (frag, alias) = parse_alias(self.frag?);

        alias.and_then(|a| ALIASES.resolve_alias(a).ok().map(|a| (frag, Some(a))))
    }

    fn to_url(&self) -> Option<Url> {
        use gix_url::Scheme;

        let (frag, resolved) = self.render_alias().unwrap_or((self.frag?, None));

        #[allow(clippy::unnecessary_unwrap)]
        let (rest, (maybe_host, delim)) = if resolved.is_some() {
            resolved
                .as_ref()
                .and_then(|r| parse_host(r).ok())
                .unwrap_or(("", (resolved.as_ref().unwrap(), "")))
        } else {
            parse_host(frag).unwrap_or(("", (frag, "")))
        };

        let (maybe_host, port) = parse_port(maybe_host)
            .ok()
            .and_then(|(_, h)| h.map(|(h, p)| (h, p.parse_to())))
            .unwrap_or((maybe_host, None));

        let host = addr::parse_dns_name(maybe_host).ok().and_then(|s| {
            if s.has_known_suffix() && maybe_host.contains('.')
                || self.user.is_some()
                || self.pass.is_some()
            {
                Some(maybe_host)
            } else {
                None
            }
        });

        let scheme: Scheme = self
            .scheme
            .unwrap_or_else(|| {
                if host.is_none() {
                    "file"
                } else if delim == ":" || self.user.is_some() && self.pass.is_none() {
                    "ssh"
                } else {
                    "https"
                }
            })
            .into();

        // special case for empty fragments, e.g. foo::my-atom
        let rest = if rest.is_empty() { frag } else { rest };

        let path = if host.is_none() {
            format!("{maybe_host}{delim}{rest}")
        } else if !rest.starts_with('/') {
            format!("/{rest}")
        } else {
            rest.into()
        };

        tracing::trace!(
            ?scheme,
            delim,
            host,
            port,
            path,
            rest,
            maybe_host,
            frag,
            ?resolved
        );

        let alternate_form = scheme == Scheme::File || scheme == Scheme::Ssh;
        let port = if scheme == Scheme::Ssh {
            tracing::warn!(
                port,
                "ignoring configured port due to an upstream parsing bug"
            );
            None
        } else {
            port
        };

        Url::from_parts(
            scheme,
            self.user.map(Into::into),
            self.pass.map(Into::into),
            host.map(Into::into),
            port,
            path.into(),
            alternate_form,
        )
        .map_err(|e| {
            tracing::debug!(?e);
            e
        })
        .ok()
    }
}

impl<'a> AtomRef<'a> {
    fn render(&self) -> Result<(Id, Option<VersionReq>), UriError> {
        let id = Id::try_from(self.id.ok_or(UriError::NoAtom)?)?;
        let version = if let Some(v) = self.version {
            VersionReq::parse(v)?.into()
        } else {
            None
        };
        Ok((id, version))
    }
}

impl<'a> TryFrom<Ref<'a>> for Uri {
    type Error = UriError;

    fn try_from(refs: Ref<'a>) -> Result<Self, Self::Error> {
        let Ref { url, atom } = refs;

        let url = url.to_url();

        let (id, version) = atom.render()?;

        tracing::trace!(?url, %id, ?version);

        Ok(Uri { url, id, version })
    }
}

impl<'a> TryFrom<UrlRef<'a>> for Url {
    type Error = UriError;

    fn try_from(refs: UrlRef<'a>) -> Result<Self, Self::Error> {
        match refs.to_url() {
            Some(url) => Ok(url),
            None => Err(UriError::NoUrl),
        }
    }
}

impl FromStr for Uri {
    type Err = UriError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let r = Ref::from(s);
        Uri::try_from(r)
    }
}

impl<'a> TryFrom<&'a str> for Uri {
    type Error = UriError;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl std::fmt::Display for Uri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let url = self
            .url
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let version = self
            .version
            .as_ref()
            .map(|v| format!("@{v}"))
            .unwrap_or_default();
        write!(f, "{}::{}{}", &url.trim_end_matches('/'), self.id, &version)
    }
}

impl Uri {
    #[must_use]
    /// Returns a reference to the Url parsed out of the Atom URI.
    pub fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }

    #[must_use]
    /// Returns the Atom identifier parsed from the URI.
    pub fn id(&self) -> &Id {
        &self.id
    }

    #[must_use]
    /// Returns the Atom version parsed from the URI.
    pub fn version(&self) -> Option<&VersionReq> {
        self.version.as_ref()
    }
}
