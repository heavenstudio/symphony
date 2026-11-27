//! The provider Google, by OpenID Connect's authorization code flow with PKCE.
//!
//! A visitor is sent to Google's authorization endpoint ([`Provider::authorize`]) with the
//! client's id, the way back, the scope `openid` alone, and a sign-in's fresh `state`, `nonce` and
//! S256 challenge ([`crate::flow::Flow`]). Google sends them back with a code, which the service
//! exchanges at Google's token endpoint with its client secret and the challenge's verifier
//! ([`Provider::exchange`], through `curl`). The answer's id token is taken as Google's word
//! because it came from Google's token endpoint over TLS, in answer to the client's secret, as
//! OpenID Connect Core 3.1.3.7 allows, so its signature is not fetched a key to check. Every claim
//! the service relies on is checked ([`subject`]): its issuer is Google, it was made for this
//! client, it holds now, it carries this sign-in's nonce, and it names a subject.
//!
//! The subject is Google's `sub`, which never changes and is never given to another account, as
//! an email can be; it is what the word says with the service's name, `google`. Nothing else of
//! the token is read: no email, no name, no group.
//!
//! The endpoints are constants ([`Provider::google`]); a test alone stands one in.

use crate::curl;
use crate::flow::Flow;
use crate::settings::Client;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use subtle::ConstantTimeEq as _;

/// The service's name in the word, the SERVICE of a profile's `google:SUBJECT` sign-in.
pub(crate) const SERVICE: &str = "google";
/// Google's authorization endpoint, which a visitor is sent to.
const AUTHORIZE: &str = "https://accounts.google.com/o/oauth2/v2/auth";
/// Google's token endpoint, where a code is exchanged.
const TOKEN: &str = "https://oauth2.googleapis.com/token";
/// The issuers Google's id tokens name.
const ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];
/// How far ahead of this machine's clock an id token may say it was issued, in seconds.
pub(crate) const SKEW: u64 = 30;
/// The longest subject taken, in bytes: OpenID Connect's bound.
const SUBJECT_BYTES: usize = 255;

/// Why an id token is not taken.
pub(crate) const MALFORMED: &str =
    "the id token is not three parts with claims of the kinds OpenID Connect gives";
pub(crate) const ISSUER: &str = "the id token was not issued by Google";
pub(crate) const AUDIENCE: &str = "the id token was made for another client";
pub(crate) const EXPIRED: &str = "the id token has expired";
pub(crate) const EARLY: &str = "the id token says it was issued ahead of this machine's clock";
pub(crate) const NONCE: &str = "the id token's nonce is not this sign-in's";
pub(crate) const SUBJECT: &str = "the id token's subject is not 1 to 255 visible ASCII characters";

/// Where the provider is reached.
#[derive(Clone, Debug)]
pub(crate) struct Provider {
    authorize: String,
    token: String,
    /// The program the code is exchanged through: `curl`, or a test's own.
    curl: String,
}

impl Provider {
    /// Google.
    pub(crate) fn google() -> Provider {
        Provider {
            authorize: AUTHORIZE.into(),
            token: TOKEN.into(),
            curl: "curl".into(),
        }
    }

    /// Google's authorization endpoint, with a test's stand-in token endpoint at `token`, reached
    /// through `curl`.
    #[cfg(test)]
    pub(crate) fn stand_in(token: &str, curl: &str) -> Provider {
        Provider {
            authorize: AUTHORIZE.into(),
            token: token.into(),
            curl: curl.into(),
        }
    }

    /// Where a visitor is sent to sign in for the sign-in under way `flow`, as `client`, to come
    /// back to `redirect`.
    pub(crate) fn authorize(&self, client: &Client, redirect: &str, flow: &Flow) -> String {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("response_type", "code")
            .append_pair("client_id", &client.id)
            .append_pair("redirect_uri", redirect)
            .append_pair("scope", "openid")
            .append_pair("state", &flow.state)
            .append_pair("nonce", &flow.nonce)
            .append_pair("code_challenge", &flow.challenge())
            .append_pair("code_challenge_method", "S256")
            .finish();
        format!("{}?{query}", self.authorize)
    }

    /// The id token Google answers `code` with, exchanged as `client` with PKCE's `verifier` for
    /// a sign-in that came back to `redirect`. It blocks while `curl` runs.
    ///
    /// # Errors
    ///
    /// `curl` failed, or the endpoint answered with no id token: said with the status and the
    /// error's code where it names one, never the secret, the code or the verifier.
    pub(crate) fn exchange(
        &self,
        client: &Client,
        redirect: &str,
        code: &str,
        verifier: &str,
    ) -> Result<String, String> {
        #[derive(Deserialize)]
        struct Answered {
            id_token: Option<String>,
        }
        let secret = client.secret.expose();
        let form = form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "authorization_code")
            .append_pair("code", code)
            .append_pair("code_verifier", verifier)
            .append_pair("redirect_uri", redirect)
            .append_pair("client_id", &client.id)
            .append_pair("client_secret", secret)
            .finish();
        let answer = curl::post(&self.curl, &self.token, &form, &[secret, code, verifier])?;
        let token = (answer.status == 200)
            .then(|| serde_json::from_str::<Answered>(&answer.body).ok())
            .flatten()
            .and_then(|answered| answered.id_token);
        token.ok_or_else(|| {
            format!(
                "Google's token endpoint answered {} with no id token{}",
                answer.status,
                answer.error()
            )
        })
    }
}

/// The subject the id token `token` names, as checked for the client `client_id` and the sign-in
/// whose nonce is `nonce`, at `now`, in seconds since 1970.
///
/// # Errors
///
/// [`MALFORMED`], [`ISSUER`], [`AUDIENCE`], [`EXPIRED`], [`EARLY`], [`NONCE`] or [`SUBJECT`],
/// each for the claim it names.
pub(crate) fn subject(
    token: &str,
    client_id: &str,
    nonce: &str,
    now: u64,
) -> Result<String, &'static str> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Audience {
        One(String),
        Many(Vec<String>),
    }
    #[derive(Deserialize)]
    struct Claims {
        iss: String,
        sub: String,
        aud: Audience,
        #[serde(default)]
        azp: Option<String>,
        exp: u64,
        iat: u64,
        #[serde(default)]
        nonce: Option<String>,
    }
    let mut parts = token.split('.');
    let (Some(_header), Some(body), Some(_signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(MALFORMED);
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(body.trim_end_matches('='))
        .ok()
        .ok_or(MALFORMED)?;
    if bytes.trim_ascii_start().first() != Some(&b'{') {
        return Err(MALFORMED);
    }
    let claims: Claims = serde_json::from_slice(&bytes).ok().ok_or(MALFORMED)?;
    if !ISSUERS.contains(&claims.iss.as_str()) {
        return Err(ISSUER);
    }
    let audiences: Vec<&str> = match &claims.aud {
        Audience::One(one) => vec![one.as_str()],
        Audience::Many(many) => many.iter().map(String::as_str).collect(),
    };
    let party = claims.azp.as_deref();
    if !audiences.contains(&client_id)
        || (audiences.len() > 1 && party != Some(client_id))
        || party.is_some_and(|party| party != client_id)
    {
        return Err(AUDIENCE);
    }
    if claims.exp <= now {
        return Err(EXPIRED);
    }
    if claims.iat > now.saturating_add(SKEW) {
        return Err(EARLY);
    }
    let ours = (claims.nonce.as_deref())
        .is_some_and(|said| bool::from(said.as_bytes().ct_eq(nonce.as_bytes())));
    if !ours {
        return Err(NONCE);
    }
    let sub = claims.sub;
    if sub.is_empty() || sub.len() > SUBJECT_BYTES || !sub.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(SUBJECT);
    }
    Ok(sub)
}

#[cfg(test)]
#[path = "google_tests.rs"]
mod tests;
