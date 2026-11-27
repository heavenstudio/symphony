//! The provider GitHub, by OAuth 2's authorization code flow with PKCE, as an OAuth app, which
//! issues no id token.
//!
//! A visitor is sent to GitHub's authorization endpoint ([`Provider::authorize`]) with the app's
//! client id, the way back, a sign-in's fresh `state` and its S256 challenge
//! ([`crate::flow::Flow`]), and no scope: the one thing the service reads, the account's `id`, is
//! public, which a token of no scope reads, where `read:user` would add private profile data nothing
//! reads. GitHub sends them back with a code, which the service exchanges at GitHub's token
//! endpoint with the app's secret and the challenge's verifier ([`Provider::exchange`]). The access
//! token it answers is used once, to ask GitHub's API whose it is ([`Provider::user`]), and is kept
//! nowhere; both calls go through `curl`, the token in no argument.
//!
//! The subject is the account's numeric `id` ([`subject`]), which never changes and is never given
//! to another account, as its `login` can be: a login given up is anyone's to take. It is what the
//! word says with the service's name, `github`. Nothing else of the answer is read: no login, no
//! email, no name, no organization.
//!
//! The endpoints are constants ([`Provider::github`]); a test alone stands them in.

use crate::curl;
use crate::flow::Flow;
use crate::settings::Client;
use serde::Deserialize;

/// The service's name in the word, the SERVICE of a profile's `github:ID` sign-in.
pub(crate) const SERVICE: &str = "github";
/// GitHub's authorization endpoint, which a visitor is sent to.
const AUTHORIZE: &str = "https://github.com/login/oauth/authorize";
/// GitHub's token endpoint, where a code is exchanged.
const TOKEN: &str = "https://github.com/login/oauth/access_token";
/// GitHub's API endpoint that says whose an access token is.
const USER: &str = "https://api.github.com/user";
/// The version of GitHub's API the answer is read in.
const API_VERSION: &str = "2022-11-28";
/// The longest access token taken, in bytes: the most GitHub says its tokens grow to.
const TOKEN_BYTES: usize = 255;

/// Why GitHub's answer of whose the token is, is not taken.
pub(crate) const NO_ID: &str = "GitHub's answer names no account id that is a whole number above 0";

/// Where GitHub is reached.
#[derive(Clone, Debug)]
pub(crate) struct Provider {
    authorize: String,
    token: String,
    user: String,
    /// The program the calls go through: `curl`, or a test's own.
    curl: String,
}

impl Provider {
    /// GitHub.
    pub(crate) fn github() -> Provider {
        Provider {
            authorize: AUTHORIZE.into(),
            token: TOKEN.into(),
            user: USER.into(),
            curl: "curl".into(),
        }
    }

    /// GitHub's authorization endpoint, with a test's stand-in token endpoint at `token` and user
    /// endpoint at `user`, reached through `curl`.
    #[cfg(test)]
    pub(crate) fn stand_in(token: &str, user: &str, curl: &str) -> Provider {
        Provider {
            authorize: AUTHORIZE.into(),
            token: token.into(),
            user: user.into(),
            curl: curl.into(),
        }
    }

    /// Where a visitor is sent to sign in for the sign-in under way `flow`, as `client`, to come
    /// back to `redirect`.
    pub(crate) fn authorize(&self, client: &Client, redirect: &str, flow: &Flow) -> String {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("client_id", &client.id)
            .append_pair("redirect_uri", redirect)
            .append_pair("state", &flow.state)
            .append_pair("code_challenge", &flow.challenge())
            .append_pair("code_challenge_method", "S256")
            .finish();
        format!("{}?{query}", self.authorize)
    }

    /// The access token GitHub answers `code` with, exchanged as `client` with PKCE's `verifier`
    /// for a sign-in that came back to `redirect`. It blocks while `curl` runs.
    ///
    /// # Errors
    ///
    /// `curl` failed, or the endpoint answered with no access token of visible ASCII (GitHub says
    /// a code it does not take `200` with an `error`): said with the status and the error's code
    /// where it names one, never the secret, the code or the verifier.
    pub(crate) fn exchange(
        &self,
        client: &Client,
        redirect: &str,
        code: &str,
        verifier: &str,
    ) -> Result<String, String> {
        #[derive(Deserialize)]
        struct Answered {
            access_token: Option<String>,
        }
        let secret = client.secret.expose();
        let form = form_urlencoded::Serializer::new(String::new())
            .append_pair("client_id", &client.id)
            .append_pair("client_secret", secret)
            .append_pair("code", code)
            .append_pair("redirect_uri", redirect)
            .append_pair("code_verifier", verifier)
            .finish();
        let answer = curl::post(&self.curl, &self.token, &form, &[secret, code, verifier])?;
        // The token goes on a line of curl's config next, so it is visible ASCII alone: no
        // newline, no space.
        let token = (answer.status == 200)
            .then(|| serde_json::from_str::<Answered>(&answer.body).ok())
            .flatten()
            .and_then(|answered| answered.access_token)
            .filter(|token| {
                !token.is_empty()
                    && token.len() <= TOKEN_BYTES
                    && token.bytes().all(|b| b.is_ascii_graphic())
            });
        token.ok_or_else(|| {
            format!(
                "GitHub's token endpoint answered {} with no access token{}",
                answer.status,
                answer.error()
            )
        })
    }

    /// What GitHub's API answers of whose the access token `token` is: the account, as JSON. It
    /// blocks while `curl` runs.
    ///
    /// # Errors
    ///
    /// `curl` failed, or the endpoint answered other than `200`: said with the status, never the
    /// token.
    pub(crate) fn user(&self, token: &str) -> Result<String, String> {
        let authorization = format!("Authorization: Bearer {token}");
        let version = format!("X-GitHub-Api-Version: {API_VERSION}");
        let headers = [
            authorization.as_str(),
            "Accept: application/vnd.github+json",
            version.as_str(),
        ];
        let answer = curl::get(&self.curl, &self.user, &headers, &[token])?;
        if answer.status == 200 {
            Ok(answer.body)
        } else {
            Err(format!("GitHub's user endpoint answered {}", answer.status))
        }
    }
}

/// The subject GitHub's answer `user` names: the account's `id`, a whole number above 0, in
/// decimal.
///
/// # Errors
///
/// [`NO_ID`]: the answer is not a JSON object, or its `id` is missing, or is not a whole number
/// above 0, as a string of digits, a fraction or a negative.
pub(crate) fn subject(user: &str) -> Result<String, &'static str> {
    #[derive(Deserialize)]
    struct User {
        id: Option<serde_json::Value>,
    }
    (serde_json::from_str::<User>(user).ok())
        .and_then(|user| user.id)
        .and_then(|id| id.as_u64())
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
        .ok_or(NO_ID)
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
