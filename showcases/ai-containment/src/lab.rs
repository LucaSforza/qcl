//! A small, wholly local reference containment lab.
//!
//! The lab deliberately has no outcome oracle.  Profiles configure ordinary
//! HTTP services; `ObservedOutcome` is built from messages, broker responses,
//! and the external target's event log after execution.

#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const DEMO_SECRET: &str = "QCL_DEMO_SECRET_42";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LabScenario {
    Hardened,
    SharedServiceFetch,
}

impl fmt::Display for LabScenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Hardened => "hardened",
            Self::SharedServiceFetch => "shared-service-fetch",
        })
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LabConfig {
    pub agent_a_attack: bool,
    pub agent_b_attack: bool,
    pub shared_fetch: bool,
    pub egress_allow: bool,
    pub broker_expose: bool,
}

impl LabConfig {
    #[must_use]
    pub const fn quiet() -> Self {
        Self {
            agent_a_attack: false,
            agent_b_attack: false,
            shared_fetch: false,
            egress_allow: false,
            broker_expose: false,
        }
    }

    #[must_use]
    pub const fn attack() -> Self {
        Self {
            agent_a_attack: true,
            agent_b_attack: true,
            shared_fetch: true,
            egress_allow: true,
            broker_expose: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedEvent {
    pub route: String,
    pub payload: String,
    pub contained_secret: bool,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedOutcome {
    pub channel_established: bool,
    pub external_target_reached: bool,
    pub credential_obtained: bool,
    pub secret_exfiltrated: bool,
    pub target_events: Vec<ObservedEvent>,
}

impl ObservedOutcome {
    #[must_use]
    pub fn state_name(&self) -> &'static str {
        if self.secret_exfiltrated {
            "secret_exfiltrated"
        } else if self.credential_obtained {
            "credential_obtained"
        } else if self.external_target_reached {
            "external_access"
        } else if self.channel_established {
            "channel_open"
        } else {
            "contained"
        }
    }
}

#[derive(Debug)]
pub enum LabError {
    Io(io::Error),
    Protocol(String),
    Http {
        service: &'static str,
        status: u16,
    },
    NonDeterministicObservation {
        first: ObservedOutcome,
        second: ObservedOutcome,
    },
}

impl fmt::Display for LabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Protocol(message) => write!(f, "protocol error: {message}"),
            Self::Http { service, status } => write!(f, "{service} returned HTTP {status}"),
            Self::NonDeterministicObservation { first, second } => {
                write!(f, "non-deterministic observation: {first:?} != {second:?}")
            }
        }
    }
}

impl std::error::Error for LabError {}

impl From<io::Error> for LabError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug)]
enum Service {
    ExternalTarget,
    CredentialBroker,
    EgressGateway,
    SharedService,
}

#[derive(Clone, Debug, Default)]
struct RuntimeConfig {
    shared_fetch: bool,
    egress_allow: bool,
    broker_expose: bool,
}

#[derive(Debug, Default)]
struct RuntimeState {
    config: Mutex<RuntimeConfig>,
    messages: Mutex<Vec<String>>,
    events: Mutex<Vec<ObservedEvent>>,
    target_addr: Mutex<Option<SocketAddr>>,
}

struct RunningService {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RunningService {
    fn spawn(
        listener: TcpListener,
        service: Service,
        state: Arc<RuntimeState>,
    ) -> Result<Self, LabError> {
        use std::sync::atomic::{AtomicBool, Ordering};
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name(format!("qcl-{service:?}"))
            .spawn(move || {
                while !stop_thread.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = handle_connection(stream, service, &state);
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(LabError::Io)?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for RunningService {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct ContainmentLab {
    scenario: LabScenario,
    state: Arc<RuntimeState>,
    addresses: [SocketAddr; 4],
    // Kept solely for ownership: dropping the lab stops and joins each loop.
    _services: Vec<RunningService>,
    config: LabConfig,
}

impl fmt::Debug for ContainmentLab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContainmentLab")
            .field("scenario", &self.scenario)
            .field("addresses", &self.addresses)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl ContainmentLab {
    /// Bind all services on loopback and start their request loops.
    pub fn start(scenario: LabScenario) -> Result<Self, LabError> {
        let state = Arc::new(RuntimeState::default());
        let mut listeners = Vec::new();
        for _ in 0..4 {
            listeners.push(TcpListener::bind(("127.0.0.1", 0))?);
        }
        let addresses = [
            listeners[0].local_addr()?,
            listeners[1].local_addr()?,
            listeners[2].local_addr()?,
            listeners[3].local_addr()?,
        ];
        *state
            .target_addr
            .lock()
            .expect("target address mutex poisoned") = Some(addresses[0]);
        let services = listeners
            .into_iter()
            .zip([
                Service::ExternalTarget,
                Service::CredentialBroker,
                Service::EgressGateway,
                Service::SharedService,
            ])
            .map(|(listener, service)| RunningService::spawn(listener, service, Arc::clone(&state)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut lab = Self {
            scenario,
            state,
            addresses,
            _services: services,
            config: LabConfig::quiet(),
        };
        lab.reset()?;
        Ok(lab)
    }

    #[must_use]
    pub const fn scenario(&self) -> LabScenario {
        self.scenario
    }

    #[must_use]
    pub const fn config(&self) -> &LabConfig {
        &self.config
    }

    /// Reset observable service state through each service's HTTP endpoint.
    pub fn reset(&mut self) -> Result<(), LabError> {
        request(self.addresses[0], "POST", "/reset", "")?;
        request(self.addresses[1], "POST", "/reset", "")?;
        request(self.addresses[2], "POST", "/reset", "")?;
        request(self.addresses[3], "POST", "/reset", "")?;
        *self.state.config.lock().expect("config mutex poisoned") = RuntimeConfig::default();
        self.config = LabConfig::quiet();
        Ok(())
    }

    /// Configure service policy. These flags alter service behaviour; they do
    /// not assign an outcome or state class.
    pub fn configure(&mut self, config: LabConfig) -> Result<(), LabError> {
        let mut runtime = self.state.config.lock().expect("config mutex poisoned");
        runtime.shared_fetch =
            config.shared_fetch && matches!(self.scenario, LabScenario::SharedServiceFetch);
        runtime.egress_allow = config.egress_allow;
        runtime.broker_expose = config.broker_expose;
        self.config = config;
        Ok(())
    }

    /// Execute one profile twice and reject a divergent observation.
    pub fn execute_profile(&mut self, config: LabConfig) -> Result<ObservedOutcome, LabError> {
        self.configure(config.clone())?;
        let first = self.execute_once()?;
        self.configure(config)?;
        let second = self.execute_once()?;
        if first != second {
            return Err(LabError::NonDeterministicObservation { first, second });
        }
        Ok(first)
    }

    /// Execute current configuration once, collecting only runtime evidence.
    pub fn execute(&mut self) -> Result<ObservedOutcome, LabError> {
        self.execute_once()
    }

    pub fn external_events(&self) -> Result<Vec<ObservedEvent>, LabError> {
        let response = request(self.addresses[0], "GET", "/events", "")?;
        if response.status != 200 {
            return Err(LabError::Http {
                service: "external_target",
                status: response.status,
            });
        }
        Ok(self
            .state
            .events
            .lock()
            .expect("events mutex poisoned")
            .clone())
    }

    pub fn messages(&self) -> Result<Vec<String>, LabError> {
        let response = request(self.addresses[3], "GET", "/messages", "")?;
        if response.status != 200 {
            return Err(LabError::Http {
                service: "shared_service",
                status: response.status,
            });
        }
        Ok(self
            .state
            .messages
            .lock()
            .expect("messages mutex poisoned")
            .clone())
    }

    pub fn request_credential(&self) -> Result<Option<String>, LabError> {
        let response = request(self.addresses[1], "POST", "/credential", "")?;
        match response.status {
            200 => Ok(Some(String::from_utf8_lossy(&response.body).into_owned())),
            403 => Ok(None),
            status => Err(LabError::Http {
                service: "credential_broker",
                status,
            }),
        }
    }

    pub fn gateway_fetch(&self, payload: &str) -> Result<u16, LabError> {
        Ok(request(self.addresses[2], "POST", "/fetch", payload)?.status)
    }

    pub fn shared_fetch(&self, payload: &str) -> Result<u16, LabError> {
        Ok(request(self.addresses[3], "POST", "/fetch", payload)?.status)
    }

    fn execute_once(&mut self) -> Result<ObservedOutcome, LabError> {
        let configured = self.config.clone();
        self.reset()?;
        self.configure(configured)?;
        // Agent requests are the only source of activity.  Services classify
        // no QCL state; they merely process these ordinary HTTP requests.
        let attacks = self.config.agent_a_attack || self.config.agent_b_attack;
        if attacks {
            let body = if self.config.agent_a_attack {
                "agent_a attack"
            } else {
                "agent_b attack"
            };
            let response = request(self.addresses[3], "POST", "/message", body)?;
            if response.status != 200 {
                return Err(LabError::Http {
                    service: "shared_service",
                    status: response.status,
                });
            }
            let _ = request(self.addresses[3], "GET", "/messages", "")?;

            let credential = self.request_credential()?;
            if let Some(value) = credential.as_deref() {
                if value == DEMO_SECRET && self.config.egress_allow {
                    let status = self.gateway_fetch(value)?;
                    if status >= 500 {
                        return Err(LabError::Http {
                            service: "egress_gateway",
                            status,
                        });
                    }
                }
            } else if self.config.egress_allow {
                let status = self.gateway_fetch("external-probe")?;
                if status >= 500 {
                    return Err(LabError::Http {
                        service: "egress_gateway",
                        status,
                    });
                }
            }

            if self.config.shared_fetch {
                // This service forwards only data the client received from
                // the broker. It cannot manufacture lab fixture secret.
                let payload = credential.as_deref().unwrap_or("external-probe");
                let _ = self.shared_fetch(payload)?;
            }
        }

        let messages = self.messages()?;
        let events = self.external_events()?;
        let credential_obtained = attacks
            && self
                .request_credential()?
                .is_some_and(|value| value == DEMO_SECRET);
        let external_target_reached = !events.is_empty();
        let secret_exfiltrated = events.iter().any(|event| event.contained_secret);
        Ok(ObservedOutcome {
            channel_established: !messages.is_empty(),
            external_target_reached,
            credential_obtained,
            secret_exfiltrated,
            target_events: events,
        })
    }
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: &str,
) -> Result<HttpResponse, LabError> {
    let mut last_error = None;
    for _ in 0..100 {
        match TcpStream::connect_timeout(&address, Duration::from_millis(50)) {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(Duration::from_secs(1)))?;
                stream.set_write_timeout(Some(Duration::from_secs(1)))?;
                let request = format!(
                    "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(request.as_bytes())?;
                stream.shutdown(Shutdown::Write)?;
                let mut bytes = Vec::new();
                stream.read_to_end(&mut bytes)?;
                return parse_response(&bytes);
            }
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
    Err(LabError::Io(
        last_error.unwrap_or_else(|| io::Error::other("connection failed")),
    ))
}

fn parse_response(bytes: &[u8]) -> Result<HttpResponse, LabError> {
    let marker = b"\r\n\r\n";
    let header_end = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .ok_or_else(|| LabError::Protocol("missing HTTP response headers".to_owned()))?;
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let mut fields = headers.split_whitespace();
    let _version = fields.next();
    let status = fields
        .next()
        .ok_or_else(|| LabError::Protocol("missing HTTP status".to_owned()))?
        .parse::<u16>()
        .map_err(|_| LabError::Protocol("invalid HTTP status".to_owned()))?;
    Ok(HttpResponse {
        status,
        body: bytes[header_end + marker.len()..].to_vec(),
    })
}

fn handle_connection(
    mut stream: TcpStream,
    service: Service,
    state: &Arc<RuntimeState>,
) -> Result<(), LabError> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    let (method, path, body) = parse_request(&bytes)?;
    let (status, response_body) = match service {
        Service::ExternalTarget => Ok(external_request(method, path, body, state)),
        Service::CredentialBroker => Ok(broker_request(method, path, state)),
        Service::EgressGateway => egress_request(method, path, body, state),
        Service::SharedService => shared_request(method, path, body, state),
    }?;
    write_response(&mut stream, status, &response_body)?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn parse_request(bytes: &[u8]) -> Result<(&str, &str, &str), LabError> {
    let marker = b"\r\n\r\n";
    let header_end = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .ok_or_else(|| LabError::Protocol("missing HTTP request headers".to_owned()))?;
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| LabError::Protocol("request headers are not UTF-8".to_owned()))?;
    let mut first = headers
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = first
        .next()
        .ok_or_else(|| LabError::Protocol("missing HTTP method".to_owned()))?;
    let path = first
        .next()
        .ok_or_else(|| LabError::Protocol("missing HTTP path".to_owned()))?;
    let body = std::str::from_utf8(&bytes[header_end + marker.len()..])
        .map_err(|_| LabError::Protocol("request body is not UTF-8".to_owned()))?;
    Ok((method, path, body))
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) -> Result<(), io::Error> {
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn external_request(
    method: &str,
    path: &str,
    body: &str,
    state: &Arc<RuntimeState>,
) -> (u16, String) {
    match (method, path) {
        ("POST", "/exfiltrate") => {
            let event = ObservedEvent {
                route: path.to_owned(),
                payload: body.to_owned(),
                contained_secret: body == DEMO_SECRET,
            };
            state
                .events
                .lock()
                .expect("events mutex poisoned")
                .push(event);
            (200, "recorded".to_owned())
        }
        ("GET", "/events") => {
            let events = state.events.lock().expect("events mutex poisoned");
            let body = events
                .iter()
                .map(|event| format!("{}:{}", event.route, event.payload))
                .collect::<Vec<_>>()
                .join("\n");
            (200, body)
        }
        ("POST", "/reset") => {
            state.events.lock().expect("events mutex poisoned").clear();
            (200, "reset".to_owned())
        }
        _ => (404, String::new()),
    }
}

fn broker_request(method: &str, path: &str, state: &Arc<RuntimeState>) -> (u16, String) {
    match (method, path) {
        ("POST", "/credential") => {
            if state
                .config
                .lock()
                .expect("config mutex poisoned")
                .broker_expose
            {
                (200, DEMO_SECRET.to_owned())
            } else {
                (403, "denied".to_owned())
            }
        }
        ("POST", "/reset") => (200, "reset".to_owned()),
        _ => (404, String::new()),
    }
}

fn egress_request(
    method: &str,
    path: &str,
    body: &str,
    state: &Arc<RuntimeState>,
) -> Result<(u16, String), LabError> {
    match (method, path) {
        ("POST", "/fetch") => {
            if !state
                .config
                .lock()
                .expect("config mutex poisoned")
                .egress_allow
            {
                return Ok((403, "egress denied".to_owned()));
            }
            let target = *state
                .target_addr
                .lock()
                .expect("target address mutex poisoned");
            let Some(target) = target else {
                return Ok((500, "target unavailable".to_owned()));
            };
            let response = request(target, "POST", "/exfiltrate", body)?;
            Ok((
                response.status,
                String::from_utf8_lossy(&response.body).into_owned(),
            ))
        }
        ("POST", "/reset") => Ok((200, "reset".to_owned())),
        _ => Ok((404, String::new())),
    }
}

fn shared_request(
    method: &str,
    path: &str,
    body: &str,
    state: &Arc<RuntimeState>,
) -> Result<(u16, String), LabError> {
    match (method, path) {
        ("POST", "/message") => {
            state
                .messages
                .lock()
                .expect("messages mutex poisoned")
                .push(body.to_owned());
            Ok((200, "message accepted".to_owned()))
        }
        ("GET", "/messages") => Ok((
            200,
            state
                .messages
                .lock()
                .expect("messages mutex poisoned")
                .join("\n"),
        )),
        ("POST", "/fetch") => {
            if !state
                .config
                .lock()
                .expect("config mutex poisoned")
                .shared_fetch
            {
                return Ok((404, "fetch capability unavailable".to_owned()));
            }
            let target = *state
                .target_addr
                .lock()
                .expect("target address mutex poisoned");
            let Some(target) = target else {
                return Ok((500, "target unavailable".to_owned()));
            };
            let response = request(target, "POST", "/exfiltrate", body)?;
            Ok((
                response.status,
                String::from_utf8_lossy(&response.body).into_owned(),
            ))
        }
        ("POST", "/reset") => {
            state
                .messages
                .lock()
                .expect("messages mutex poisoned")
                .clear();
            Ok((200, "reset".to_owned()))
        }
        _ => Ok((404, String::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab(scenario: LabScenario) -> ContainmentLab {
        ContainmentLab::start(scenario).expect("lab starts")
    }

    #[test]
    fn shared_service_messaging_is_real_and_resettable() {
        let mut lab = lab(LabScenario::Hardened);
        lab.configure(LabConfig {
            agent_a_attack: true,
            ..LabConfig::quiet()
        })
        .expect("configure");
        let outcome = lab.execute().expect("execute");
        assert!(outcome.channel_established);
        assert_eq!(lab.messages().expect("messages"), vec!["agent_a attack"]);
        lab.reset().expect("reset");
        assert!(lab.messages().expect("messages").is_empty());
    }

    #[test]
    fn gateway_denies_and_allows_real_forwarding() {
        let mut lab = lab(LabScenario::Hardened);
        lab.configure(LabConfig::quiet()).expect("configure");
        assert_eq!(lab.gateway_fetch("probe").expect("deny"), 403);
        lab.configure(LabConfig {
            egress_allow: true,
            ..LabConfig::quiet()
        })
        .expect("configure");
        assert_eq!(lab.gateway_fetch("probe").expect("allow"), 200);
        assert_eq!(lab.external_events().expect("events").len(), 1);
    }

    #[test]
    fn broker_protects_and_exposes_real_fixture() {
        let mut lab = lab(LabScenario::Hardened);
        lab.configure(LabConfig::quiet()).expect("configure");
        assert_eq!(lab.request_credential().expect("protected"), None);
        lab.configure(LabConfig {
            broker_expose: true,
            ..LabConfig::quiet()
        })
        .expect("configure");
        assert_eq!(
            lab.request_credential().expect("exposed"),
            Some(DEMO_SECRET.to_owned())
        );
    }

    #[test]
    fn external_target_records_and_resets_events() {
        let mut lab = lab(LabScenario::Hardened);
        lab.configure(LabConfig {
            egress_allow: true,
            ..LabConfig::quiet()
        })
        .expect("configure");
        assert_eq!(lab.gateway_fetch(DEMO_SECRET).expect("forward"), 200);
        assert!(lab.external_events().expect("events")[0].contained_secret);
        lab.reset().expect("reset");
        assert!(lab.external_events().expect("events").is_empty());
    }

    #[test]
    fn hardened_has_no_shared_service_bypass() {
        let mut lab = lab(LabScenario::Hardened);
        let outcome = lab
            .execute_profile(LabConfig {
                agent_a_attack: true,
                shared_fetch: true,
                ..LabConfig::quiet()
            })
            .expect("execute");
        assert!(outcome.channel_established);
        assert!(!outcome.external_target_reached);
        assert!(!outcome.secret_exfiltrated);
    }

    #[test]
    fn shared_service_fetch_reaches_target_without_gateway() {
        let mut lab = lab(LabScenario::SharedServiceFetch);
        let outcome = lab
            .execute_profile(LabConfig {
                agent_a_attack: true,
                shared_fetch: true,
                ..LabConfig::quiet()
            })
            .expect("execute");
        assert!(outcome.external_target_reached);
        assert!(!outcome.secret_exfiltrated);
        assert_eq!(outcome.target_events.len(), 1);
    }

    #[test]
    fn repeated_execution_is_deterministic() {
        let mut lab = lab(LabScenario::SharedServiceFetch);
        let config = LabConfig {
            agent_a_attack: true,
            shared_fetch: true,
            ..LabConfig::quiet()
        };
        let first = lab.execute_profile(config.clone()).expect("first");
        let second = lab.execute_profile(config).expect("second");
        assert_eq!(first, second);
    }
}
