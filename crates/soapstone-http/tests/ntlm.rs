use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, Mac};
use md4::{Digest, Md4};
use md5::Md5;
use ntlmclient::{ChallengeMessage, Flags, Message, TargetInfoEntry, TargetInfoType};
use soapstone_http::{Auth, RequestOptions, SoapCall, SoapVersion, WsSecurity, client, send};

const SERVER_CHALLENGE: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// Headers plus a Content-Length body, from a stream that may carry several requests.
fn read_request(stream: &mut TcpStream) -> String {
    let mut raw = Vec::new();
    let mut byte = [0u8; 1];
    while !raw.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap() == 0 {
            break;
        }
        raw.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&raw).into_owned();
    let len: usize = header(&head, "content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).unwrap();
    head + &String::from_utf8_lossy(&body)
}

fn header(raw: &str, name: &str) -> Option<String> {
    raw.split("\r\n\r\n").next()?.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim().to_string())
    })
}

/// Accepts ONE connection. With `first_reply`, answers the first request with it. Without, runs the NTLM
/// handshake: 401 + Type 2 challenge (keep-alive), then reads the second request from the same connection
/// and answers 200 `<ok/>`. Sends every raw request it read through the channel.
fn serve_ntlm(first_reply: Option<Vec<u8>>) -> (String, mpsc::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut requests = vec![read_request(&mut stream)];
        match first_reply {
            Some(reply) => stream.write_all(&reply).unwrap(),
            None => {
                let challenge = Message::Challenge(ChallengeMessage {
                    target_name: "DOMAIN".into(),
                    flags: Flags::NEGOTIATE_UNICODE
                        | Flags::NEGOTIATE_NTLM
                        | Flags::REQUEST_TARGET
                        | Flags::NEGOTIATE_TARGET_INFO
                        | Flags::TARGET_TYPE_DOMAIN,
                    challenge: SERVER_CHALLENGE,
                    context: (0, 0),
                    target_information: vec![
                        TargetInfoEntry::from_string(TargetInfoType::NtDomain, "DOMAIN"),
                        TargetInfoEntry { entry_type: TargetInfoType::Terminator, data: vec![] },
                    ],
                    os_version: Default::default(),
                });
                let b64 = STANDARD.encode(challenge.to_bytes().unwrap());
                let reply = format!("HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM {b64}\r\nContent-Length: 5\r\n\r\nnope!");
                stream.write_all(reply.as_bytes()).unwrap();
                requests.push(read_request(&mut stream));
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n<ok/>").unwrap();
            }
        }
        let _ = tx.send(requests);
    });
    (format!("http://127.0.0.1:{port}/svc"), rx)
}

fn ntlm_call(url: &str) -> SoapCall {
    SoapCall {
        endpoint: url.into(),
        soap_version: SoapVersion::V11,
        soap_action: None,
        headers: vec![],
        auth: Auth::Ntlm { username: "{{u}}".into(), password: "s3cret".into(), domain: "DOMAIN".into() },
        body: "<x/>".into(),
        ws_security: WsSecurity::default(),
        // A client that opened a second connection would wait for an accept that never comes.
        options: RequestOptions { timeout_seconds: 5, ..Default::default() },
    }
}

fn vars() -> HashMap<String, String> {
    HashMap::from([("u".to_string(), "alice".to_string())])
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[tokio::test]
async fn ntlm_handshake_uses_one_connection_and_a_valid_ntlmv2_proof() {
    let (url, rx) = serve_ntlm(None);
    let response = send(&client(), &ntlm_call(&url), &vars()).await.unwrap();
    assert_eq!((response.status, response.body.as_str()), (200, "<ok/>"));
    let requests = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|r| r.starts_with("POST /svc ") && r.ends_with("<x/>")), "{requests:?}");
    assert!(header(&requests[0], "authorization").unwrap().starts_with("NTLM TlRMTVNTUAAB"), "{}", requests[0]);

    let type3 = STANDARD.decode(header(&requests[1], "authorization").unwrap().strip_prefix("NTLM ").unwrap()).unwrap();
    let Message::Authenticate(m) = Message::try_from(type3.as_slice()).unwrap() else { panic!("not a Type 3 message") };
    assert_eq!((m.user_name.as_str(), m.domain_name.as_str()), ("alice", "DOMAIN"));
    // NTLMv2, computed independently: NTOWFv2 = HMAC-MD5(MD4(UTF-16LE(password)), UTF-16LE(UPPER(user) + domain)),
    // NTProofStr = HMAC-MD5(NTOWFv2, server challenge + the rest of the NT response).
    let mut mac = Hmac::<Md5>::new_from_slice(&Md4::digest(utf16("s3cret"))).unwrap();
    mac.update(&utf16("ALICE"));
    mac.update(&utf16("DOMAIN"));
    let ntowf = mac.finalize().into_bytes();
    let mut mac = Hmac::<Md5>::new_from_slice(&ntowf).unwrap();
    mac.update(&SERVER_CHALLENGE);
    mac.update(&m.ntlm_response[16..]);
    assert_eq!(&mac.finalize().into_bytes()[..], &m.ntlm_response[..16]);
}

#[tokio::test]
async fn ntlm_without_a_challenge_returns_the_server_response() {
    let replies: [(&[u8], u16); 2] = [
        (b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\n<fine/>", 200),
        (b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", 401),
    ];
    for (reply, status) in replies {
        let (url, rx) = serve_ntlm(Some(reply.to_vec()));
        let response = send(&client(), &ntlm_call(&url), &vars()).await.unwrap();
        assert_eq!(response.status, status);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap().len(), 1);
    }
}
