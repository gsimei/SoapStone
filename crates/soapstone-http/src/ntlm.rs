//! NTLMv2 messages for `Authorization: NTLM <base64>`.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ntlmclient::{Credentials, Flags, Message, NegotiateMessage};
use reqwest::header::{HeaderMap, WWW_AUTHENTICATE};

const WORKSTATION: &str = "SOAPSTONE";

/// Type 1 (negotiate) message.
pub(crate) fn negotiate() -> String {
    let message = Message::Negotiate(NegotiateMessage {
        flags: Flags::NEGOTIATE_UNICODE | Flags::REQUEST_TARGET | Flags::NEGOTIATE_NTLM,
        supplied_domain: String::new(),
        supplied_workstation: String::new(),
        os_version: Default::default(),
    });
    STANDARD.encode(message.to_bytes().expect("negotiate message with empty names"))
}

/// The Type 2 (challenge) bytes of a `WWW-Authenticate: NTLM <base64>` header, if the server sent one.
pub(crate) fn challenge(headers: &HeaderMap) -> Option<Vec<u8>> {
    headers.get_all(WWW_AUTHENTICATE).iter().filter_map(|v| v.to_str().ok()).find_map(|value| {
        let (scheme, token) = value.trim().split_once(' ')?;
        if scheme.eq_ignore_ascii_case("NTLM") { STANDARD.decode(token.trim()).ok() } else { None }
    })
}

/// Type 3 (authenticate) message answering `challenge` with an NTLMv2 response; None when it is not a
/// valid Type 2 message.
pub(crate) fn authenticate(challenge: &[u8], username: &str, password: &str, domain: &str) -> Option<String> {
    let Ok(Message::Challenge(challenge)) = Message::try_from(challenge) else { return None };
    let target_info: Vec<u8> = challenge.target_information.iter().flat_map(|e| e.to_bytes()).collect();
    let creds = Credentials { username: username.into(), password: password.into(), domain: domain.into() };
    let response = ntlmclient::respond_challenge_ntlm_v2(challenge.challenge, &target_info, ntlmclient::get_ntlm_time(), &creds);
    let message = response.to_message(&creds, WORKSTATION, Flags::NEGOTIATE_UNICODE | Flags::NEGOTIATE_NTLM);
    Some(STANDARD.encode(message.to_bytes().ok()?))
}
