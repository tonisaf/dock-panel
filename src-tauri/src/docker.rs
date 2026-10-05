//! Docker containers in the panel through the `docker` command line tool
//! (Docker Desktop puts it on PATH): which containers exist and in what state,
//! and start / stop / restart.
//!
//! `docker ps -a --format '{{json .}}'` prints one JSON object per line. Containers are
//! addressed by their hex id only, checked before it goes into a command line,
//! so the webview cannot smuggle in an option or reach anything by name.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

const LIST_TIMEOUT: Duration = Duration::from_secs(8);
/// `docker stop` waits up to 10 s for the container before killing it.
const ACTION_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    /// Short id, what `start` / `stop` / `restart` take.
    id: String,
    name: String,
    image: String,
    /// `running`, `exited`, `paused`, `restarting`, `created`, ...
    state: String,
    /// Docker's own wording: "Up 3 hours (healthy)", "Exited (0) 2 days ago".
    status: String,
    /// Published ports as "8080→80", without repeats for IPv4 / IPv6.
    ports: Vec<String>,
    /// Docker Compose project the container belongs to.
    project: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// The daemon answers.
    running: bool,
    containers: Vec<Container>,
    /// The CLI is missing or the daemon is not reachable.
    error: Option<String>,
}

enum Failure {
    /// No `docker` on PATH.
    Missing,
    Failed(String),
}

impl Failure {
    fn message(&self) -> String {
        match self {
            Failure::Missing => "Не найден docker: установите Docker Desktop".into(),
            Failure::Failed(text) => text.clone(),
        }
    }
}

fn docker(args: &[&str], timeout: Duration) -> Result<String, Failure> {
    run("docker", args, timeout)
}

/// Runs `program` and returns its stdout; kills it after `timeout`.
fn run(program: &str, args: &[&str], timeout: Duration) -> Result<String, Failure> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Failure::Missing
        } else {
            Failure::Failed(format!("Не удалось запустить docker: {e}"))
        }
    })?;

    // Read both pipes on their own threads so a full pipe cannot stall the child.
    fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        })
    }
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(40)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let stdout = out.and_then(|h| h.join().ok()).unwrap_or_default();
    let stderr = err.and_then(|h| h.join().ok()).unwrap_or_default();

    match status {
        Some(s) if s.success() => Ok(stdout),
        Some(_) => Err(Failure::Failed(last_line(
            &stderr,
            "docker завершился с ошибкой",
        ))),
        None => Err(Failure::Failed("docker не ответил вовремя".into())),
    }
}

/// The last non-empty line: where Docker puts the actual reason.
fn last_line(text: &str, fallback: &str) -> String {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(fallback)
        .trim()
        .to_string()
}

/// Whether the failure means the daemon is down rather than something wrong with the command.
fn daemon_down(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("cannot connect to the docker daemon")
        || t.contains("error during connect")
        || t.contains("docker daemon is not running")
        || t.contains("pipe/docker")
        || t.contains("dockerdesktoplinuxengine")
}

/// "0.0.0.0:8080->80/tcp, [::]:8080->80/tcp, 5432/tcp" → ["8080→80"]; unpublished ports are skipped.
fn published_ports(ports: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in ports.split(',') {
        let Some((host, container)) = part.trim().split_once("->") else {
            continue;
        };
        let host_port = host.rsplit(':').next().unwrap_or(host);
        let container_port = container.split('/').next().unwrap_or(container);
        let label = if host_port == container_port {
            host_port.to_string()
        } else {
            format!("{host_port}→{container_port}")
        };
        if !out.contains(&label) {
            out.push(label);
        }
    }
    out
}

/// The Compose project out of `Labels` ("a=b,com.docker.compose.project=shop,c=d").
fn compose_project(labels: &str) -> Option<String> {
    labels
        .split(',')
        .find_map(|l| l.strip_prefix("com.docker.compose.project="))
        .filter(|p| !p.is_empty())
        .map(str::to_string)
}

fn parse_containers(output: &str) -> Vec<Container> {
    let mut containers: Vec<Container> = output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .filter_map(|c| {
            let id = c["ID"].as_str()?.to_string();
            // `Names` can list several, comma separated; the first is the container's own.
            let name = c["Names"]
                .as_str()?
                .split(',')
                .next()?
                .trim_start_matches('/')
                .to_string();
            Some(Container {
                id,
                name,
                image: c["Image"].as_str().unwrap_or("").to_string(),
                state: c["State"].as_str().unwrap_or("").to_string(),
                status: c["Status"].as_str().unwrap_or("").to_string(),
                ports: published_ports(c["Ports"].as_str().unwrap_or("")),
                project: compose_project(c["Labels"].as_str().unwrap_or("")),
            })
        })
        .collect();
    // Running first, then by name.
    containers.sort_by(|a, b| {
        (b.state == "running")
            .cmp(&(a.state == "running"))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    containers
}

/// A container id as `docker ps` prints it: 12 to 64 lowercase hex digits.
fn valid_id(id: &str) -> bool {
    (12..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[tauri::command]
pub async fn docker_overview() -> Overview {
    let result = tauri::async_runtime::spawn_blocking(|| {
        docker(&["ps", "-a", "--format", "{{json .}}"], LIST_TIMEOUT)
    })
    .await;
    match result {
        Ok(Ok(output)) => Overview {
            running: true,
            containers: parse_containers(&output),
            error: None,
        },
        Ok(Err(failure)) => {
            let message = failure.message();
            let down = matches!(failure, Failure::Failed(_)) && daemon_down(&message);
            Overview {
                running: false,
                containers: Vec::new(),
                error: Some(if down {
                    "Docker не запущен".into()
                } else {
                    message
                }),
            }
        }
        Err(e) => Overview {
            running: false,
            containers: Vec::new(),
            error: Some(e.to_string()),
        },
    }
}

/// `start`, `stop` or `restart` for one container.
#[tauri::command]
pub async fn docker_action(id: String, action: String) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Неверный id контейнера".into());
    }
    let verb = match action.as_str() {
        "start" => "start",
        "stop" => "stop",
        "restart" => "restart",
        _ => return Err("Неизвестное действие".into()),
    };
    tauri::async_runtime::spawn_blocking(move || {
        docker(&[verb, &id], ACTION_TIMEOUT)
            .map(|_| ())
            .map_err(|f| f.message())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = r#"{"ID":"a1b2c3d4e5f6","Names":"db","Image":"postgres:16","State":"exited","Status":"Exited (0) 2 days ago","Ports":"","Labels":"com.docker.compose.project=shop,x=y"}
{"ID":"0123456789ab","Names":"web","Image":"nginx","State":"running","Status":"Up 3 hours","Ports":"0.0.0.0:8080->80/tcp, [::]:8080->80/tcp, 0.0.0.0:3000->3000/tcp, 5432/tcp","Labels":""}
not json
{"ID":"ffffffffffff","Names":"api,other/alias","Image":"api","State":"running","Status":"Up 1 hour (healthy)","Ports":"","Labels":"a=b"}
"#;

    #[test]
    fn parses_ps_output_running_first() {
        let list = parse_containers(PS);
        let names: Vec<&str> = list.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["api", "web", "db"]);
        let web = &list[1];
        assert_eq!(web.id, "0123456789ab");
        assert_eq!(web.ports, ["8080→80", "3000"]);
        assert_eq!(web.project, None);
        let db = &list[2];
        assert_eq!(db.state, "exited");
        assert_eq!(db.project.as_deref(), Some("shop"));
    }

    #[test]
    fn ignores_unusable_lines() {
        assert!(parse_containers("").is_empty());
        assert!(parse_containers("{\"Names\":\"x\"}\n[]\n").is_empty());
    }

    #[test]
    fn shortens_ports() {
        assert_eq!(published_ports("127.0.0.1:5432->5432/tcp"), ["5432"]);
        assert_eq!(
            published_ports("0.0.0.0:9000-9001->9000-9001/tcp"),
            ["9000-9001"]
        );
        assert!(published_ports("80/tcp, 443/tcp").is_empty());
    }

    #[test]
    fn container_ids_are_hex_only() {
        assert!(valid_id("0123456789ab"));
        assert!(valid_id(&"f".repeat(64)));
        assert!(!valid_id("short"));
        assert!(!valid_id(&"a".repeat(65)));
        assert!(!valid_id("--all--------"));
        assert!(!valid_id("web"));
        assert!(!valid_id("0123456789AB"));
        assert!(!valid_id("0123456789ab; rm"));
    }

    #[test]
    fn a_missing_program_is_reported() {
        let r = run("definitely-not-a-program-xyz", &[], Duration::from_secs(1));
        assert!(matches!(r, Err(Failure::Missing)));
    }

    #[cfg(unix)]
    #[test]
    fn returns_stdout_and_the_last_stderr_line() {
        assert_eq!(
            run("sh", &["-c", "echo hi"], Duration::from_secs(5))
                .ok()
                .as_deref(),
            Some("hi\n")
        );
        let r = run(
            "sh",
            &["-c", "echo first >&2; echo reason >&2; exit 1"],
            Duration::from_secs(5),
        );
        assert!(matches!(r, Err(Failure::Failed(t)) if t == "reason"));
    }

    #[cfg(unix)]
    #[test]
    fn kills_a_command_that_does_not_answer() {
        let started = Instant::now();
        let r = run("sleep", &["30"], Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(matches!(r, Err(Failure::Failed(t)) if t.contains("не ответил")));
    }

    #[test]
    fn recognises_a_stopped_daemon() {
        assert!(daemon_down("error during connect: Get \"http://%2F%2F.%2Fpipe%2Fdocker_engine/v1.45/containers/json\""));
        assert!(daemon_down("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"));
        assert!(!daemon_down(
            "Error response from daemon: No such container: x"
        ));
    }
}
