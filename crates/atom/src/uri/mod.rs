//! # Atom URI Format
//!
//! This module provides comprehensive parsing and handling of Atom URIs, which are
//! used to reference atoms from various sources including Git repositories, local
//! paths, and external URLs.
//!
//! ## URI Format
//!
//! Atom URIs follow this general format:
//! ```text
//! [scheme://][alias:][url-fragment::]atom-id[@version]
//! ```
//!
//! ### Components
//!
//! - **scheme** - Optional protocol (e.g., `https://`, `ssh://`, `file://`)
//! - **alias** - Optional user-configurable URL shortener (e.g., `gh` for GitHub)
//! - **url-fragment** - Optional path within the repository
//! - **atom-id** - Required atom identifier (Unicode string)
//! - **version** - Optional version requirement (e.g., `@1.0.0`, `@^1.0`)
//!
//! ## Key Types
//!
//! - [`Uri`] - The main parsed URI structure
//! - [`AliasedUrl`] - URL with optional alias resolution
//! - [`UriError`] - Errors that can occur during URI parsing
//!
//! ## Alias System
//!
//! Aliases provide a convenient way to shorten common URLs. They are configured
//! in the Eka configuration file and can reference full URLs or other aliases.
//!
//! ### Alias Examples
//! - `gh:owner/repo::my-atom` → `https://github.com/owner/repo::my-atom`
//! - `work:repo::my-atom` → `https://github.com/my-work-org/repo::my-atom`
//! - `local::my-atom` → `file:///path/to/repo::my-atom`
//!
//! ## URI Examples
//!
//! ```rust,no_run
//! use atom::uri::Uri;
//!
//! // Simple atom reference
//! let uri: Uri = "my-atom".parse().unwrap();
//! assert_eq!(uri.tag().to_string(), "my-atom");
//!
//! // Atom with version
//! let uri: Uri = "my-atom@^1.0.0".parse().unwrap();
//! assert_eq!(uri.tag().to_string(), "my-atom");
//!
//! // GitHub reference with alias
//! let uri: Uri = "gh:user/repo::my-atom".parse().unwrap();
//! assert_eq!(uri.url().unwrap().host().unwrap(), "github.com");
//!
//! // Direct URL reference
//! let uri: Uri = "https://github.com/user/repo::my-atom".parse().unwrap();
//! assert_eq!(uri.url().unwrap().host().unwrap(), "github.com");
//!
//! // Local file reference
//! let uri: Uri = "file:///path/to/repo::my-atom".parse().unwrap();
//! assert_eq!(uri.url().unwrap().scheme, "file".into());
//! ```
//!
//! ## Error Handling
//!
//! The URI parser provides detailed error messages for common issues:
//! - Invalid atom IDs (wrong characters, too long, etc.)
//! - Unknown aliases
//! - Malformed URLs
//! - Invalid version specifications
//! - Missing required components

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Display;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::LazyLock;

use gix::Url;
use nom::branch::alt;
use nom::bytes::complete::{tag, take_until};
use nom::character::complete::digit1;
use nom::combinator::{all_consuming, map, not, opt, peek, rest, verify};
use nom::sequence::{separated_pair, tuple};
use nom::{IResult, ParseTo};
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::id::AtomTag;
use crate::id::Error;

#[cfg(test)]
mod tests;

//================================================================================================
// Statics
//================================================================================================

static ALIASES: LazyLock<Aliases> = LazyLock::new(|| Aliases(config::CONFIG.aliases()));

//================================================================================================
// Types
//================================================================================================

/// A URL that may contain an alias to be resolved.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
#[cfg_attr(test, derive(Serialize, Deserialize))]
pub struct AliasedUrl {
    url: Url,
    r#ref: Option<String>,
}

/// Represents the parsed components of an Atom URI.
///
/// It is typically created through the `FromStr` implementation, not constructed directly.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Uri {
    /// The URL to the repository containing the Atom.
    url: Option<Url>,
    /// The Atom's ID.
    tag: AtomTag,
    /// The requested Atom version.
    version: Option<VersionReq>,
}

/// Represents either an Atom URI or an aliased URL component.
///
/// When built through the `FromStr` implementation, aliases are resolved.
#[derive(Debug, Clone)]
pub enum UriOrUrl {
    /// Atom URI variant
    Atom(Uri),
    /// URL variant
    Pin(AliasedUrl),
}

/// An error encountered when constructing the concrete types from an Atom URI.
#[derive(Error, Debug)]
pub enum UriError {
    /// The Atom identifier is missing, but required.
    #[error("Missing the required Atom ID in URI")]
    NoAtom,
    /// There is no alias in the configuration matching the one given in the URI.
    #[error("The passed alias does not exist: {0}")]
    NoAlias(String),
    /// The Url is invalid.
    #[error("Parsing URL failed")]
    NoUrl,
    /// Malformed atom tag.
    #[error(transparent)]
    BadTag(#[from] Error),
    /// The version requested is not valid.
    #[error(transparent)]
    InvalidVersionReq(#[from] semver::Error),
    /// The Url did not parse correctly.
    #[error(transparent)]
    UrlParse(#[from] gix::url::parse::Error),
    /// The Url did not parse correctly.
    #[error(transparent)]
    UrlParser(#[from] url::ParseError),
}

#[derive(Debug)]
struct Aliases(&'static HashMap<&'static str, &'static str>);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(Serialize, Deserialize))]
struct AtomRef<'a> {
    /// The specific Atom within the repository.
    tag: Option<&'a str>,
    /// The version of the Atom, if specified.
    version: Option<&'a str>,
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

type UrlPrefix<'a> = (Option<&'a str>, Option<&'a str>, Option<&'a str>);

//================================================================================================
// Impls
//================================================================================================

impl AliasedUrl {
    /// Returns a reference to the optional git ref.
    pub fn r#ref(&self) -> Option<&String> {
        self.r#ref.as_ref()
    }

    /// Returns a reference to the underlying URL.
    pub fn url(&self) -> &Url {
        &self.url
    }
}

impl Display for AliasedUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let AliasedUrl { url, r#ref } = self;
        if let Some(r) = r#ref {
            if r.is_empty() {
                url.fmt(f)
            } else {
                write!(f, "{}^{}", url, r)
            }
        } else {
            url.fmt(f)
        }
    }
}

impl FromStr for AliasedUrl {
    type Err = UriError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let r#ref: Option<String>;
        let url_str: &str;
        if let Some((f, r)) = s.split_once('^') {
            r#ref = Some(r.to_owned());
            url_str = f;
        } else {
            r#ref = None;
            url_str = s;
        }
        let url = UrlRef::from(url_str);
        let url = url.to_url()?;

        Ok(AliasedUrl { url, r#ref })
    }
}

impl TryFrom<&str> for AliasedUrl {
    type Error = UriError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        FromStr::from_str(s)
    }
}

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

impl<'a> AtomRef<'a> {
    fn render(&self) -> Result<(AtomTag, Option<VersionReq>), UriError> {
        let tag = AtomTag::try_from(self.tag.ok_or(UriError::NoAtom)?)?;
        let version = if let Some(v) = self.version {
            VersionReq::parse(v)?.into()
        } else {
            None
        };
        Ok((tag, version))
    }
}

impl<'a> From<&'a str> for AtomRef<'a> {
    fn from(s: &'a str) -> Self {
        let (tag, version) = match split_at(s) {
            Ok((rest, Some(atom))) => (Some(atom), not_empty(rest)),
            Ok((rest, None)) => (not_empty(rest), None),
            _ => (None, None),
        };

        AtomRef { tag, version }
    }
}

impl<'a> From<&'a str> for Ref<'a> {
    /// Parses a string slice into a `Ref`.
    ///
    /// This is the primary way to create a `Ref` instance.
    fn from(input: &'a str) -> Self {
        parse(input)
    }
}

impl Display for Uri {
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
        write!(
            f,
            "{}::{}{}",
            &url.trim_end_matches('/'),
            self.tag,
            &version
        )
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

impl Display for UriOrUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UriOrUrl::Atom(uri) => uri.fmt(f),
            UriOrUrl::Pin(url) => url.fmt(f),
        }
    }
}

impl FromStr for UriOrUrl {
    type Err = UriError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        if s.contains("::") {
            s.parse::<Uri>().map(UriOrUrl::Atom)
        } else {
            s.parse::<AliasedUrl>().map(UriOrUrl::Pin)
        }
    }
}

impl Uri {
    /// Returns the Atom identifier parsed from the URI.
    #[must_use]
    pub fn tag(&self) -> &AtomTag {
        &self.tag
    }

    /// Returns a reference to the Url parsed out of the Atom URI.
    #[must_use]
    pub fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }

    /// Returns the Atom version parsed from the URI.
    #[must_use]
    pub fn version(&self) -> Option<&VersionReq> {
        self.version.as_ref()
    }
}

impl<'a> TryFrom<Ref<'a>> for Uri {
    type Error = UriError;

    fn try_from(refs: Ref<'a>) -> Result<Self, Self::Error> {
        let Ref { url, atom } = refs;

        let url = url.to_url().ok();

        let (id, version) = atom.render()?;

        tracing::trace!(?url, %id, ?version);

        Ok(Uri {
            url,
            tag: id,
            version,
        })
    }
}

impl<'a> TryFrom<UrlRef<'a>> for Url {
    type Error = UriError;

    fn try_from(refs: UrlRef<'a>) -> Result<Self, Self::Error> {
        refs.to_url()
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

impl<'a> UrlRef<'a> {
    fn render_alias(&self) -> Option<(&str, Option<Cow<'static, str>>)> {
        let (frag, alias) = parse_alias(self.frag?);

        alias.and_then(|a| ALIASES.resolve_alias(a).ok().map(|a| (frag, Some(a))))
    }

    fn to_url(&self) -> Result<Url, UriError> {
        use gix::url::Scheme;

        let (frag, resolved) = self
            .render_alias()
            .unwrap_or((self.frag.unwrap_or(""), None));

        if frag.is_empty() && resolved.is_none() {
            return Err(UriError::NoUrl);
        }

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

        let rest = if !frag.contains(rest) && !frag.is_empty() {
            format!("{}/{}", rest, frag)
        } else {
            rest.into()
        };

        let path = if host.is_none() {
            format!("{maybe_host}{delim}{rest}")
        } else if !rest.starts_with('/') {
            format!("/{rest}")
        } else {
            rest.to_owned()
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

        let alternate_form = scheme == Scheme::File;
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
        .map_err(Into::into)
    }
}

//================================================================================================
// Functions
//================================================================================================

fn empty_none<'a>((rest, opt): (&'a str, Option<&'a str>)) -> (&'a str, Option<&'a str>) {
    (rest, opt.and_then(not_empty))
}

fn first_path(input: &str) -> IResult<&str, (&str, &str)> {
    tuple((
        verify(take_until("/"), |h: &str| {
            !h.contains(':') || parse_port(h).ok().and_then(|(_, p)| p).is_some()
        }),
        tag("/"),
    ))(input)
}

fn not_empty(input: &str) -> Option<&str> {
    if input.is_empty() { None } else { Some(input) }
}

fn opt_split<'a>(input: &'a str, delim: &str) -> IResult<&'a str, Option<&'a str>> {
    opt(map(tuple((take_until(delim), tag(delim))), |(url, _)| url))(input).map(empty_none)
}

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
        atom.tag,
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

fn scheme(input: &str) -> IResult<&str, Option<&str>> {
    opt_split(input, "://")
}

fn split_at(input: &str) -> IResult<&str, Option<&str>> {
    opt_split(input, "@")
}

fn split_colon(input: &str) -> IResult<&str, Option<&str>> {
    opt_split(input, ":")
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

fn url(input: &str) -> IResult<&str, Option<&str>> {
    opt_split(input, "::")
}
