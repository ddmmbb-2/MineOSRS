#[derive(Clone)]
struct AppState {
    db: Arc<StdMutex<Connection>>,
    instances: Arc<RwLock<HashMap<String, Arc<Instance>>>>,
    token: Arc<String>,
    password_set: Arc<AtomicBool>,
    sessions: Arc<StdMutex<HashMap<String, Instant>>>,
    login_attempts: Arc<StdMutex<(u32, Instant)>>,
    data_dir: Arc<PathBuf>,
    jobs: Arc<RwLock<HashMap<String, downloads::Job>>>,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServerConfig {
    id: String,
    name: String,
    port: u16,
    xms_mb: u32,
    xmx_mb: u32,
    java_command: String,
    auto_start: bool,
    auto_restart: bool,
    #[serde(default = "default_core")]
    core_type: String,
    #[serde(default)]
    mc_version: String,
    #[serde(default)]
    core_build: String,
}

// Edits to an existing Server never change the core or its Minecraft version.
#[derive(Debug, Deserialize)]
struct UpdateServer {
    name: String,
    port: u16,
    xms_mb: u32,
    xmx_mb: u32,
    java_command: String,
    auto_start: bool,
    auto_restart: bool,
}

#[derive(Debug, Deserialize)]
struct NewServer {
    name: String,
    port: u16,
    xms_mb: u32,
    xmx_mb: u32,
    #[serde(default = "default_java")]
    java_command: String,
    #[serde(default)]
    auto_start: bool,
    #[serde(default)]
    auto_restart: bool,
    #[serde(default = "default_core")]
    core_type: String,
    #[serde(default)]
    mc_version: String,
    #[serde(default)]
    core_build: String,
}

fn default_core() -> String { "manual".into() }

fn default_java() -> String {
    "java".into()
}

#[derive(Serialize)]
struct ServerView {
    #[serde(flatten)]
    config: ServerConfig,
    status: &'static str,
    message: Option<String>,
    eula_accepted: bool,
    core_ready: bool,
    folder_path: String,
}

struct Instance {
    config: RwLock<ServerConfig>,
    process: Mutex<ProcessState>,
    logs: Mutex<VecDeque<String>>,
    maintenance: AtomicBool,
}

struct ProcessState {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    status: &'static str,
    requested_stop: bool,
    pending_restart: bool,
    starts_in_window: VecDeque<Instant>,
    message: Option<String>,
}

impl Instance {
    fn new(config: ServerConfig) -> Self {
        Self {
            config: RwLock::new(config),
            process: Mutex::new(ProcessState {
                child: None,
                stdin: None,
                status: "stopped",
                requested_stop: false,
                pending_restart: false,
                starts_in_window: VecDeque::new(),
                message: None,
            }),
            logs: Mutex::new(VecDeque::new()),
            maintenance: AtomicBool::new(false),
        }
    }

    async fn log(&self, line: impl Into<String>) {
        let mut logs = self.logs.lock().await;
        logs.push_back(line.into());
        while logs.len() > MAX_LOG_LINES {
            logs.pop_front();
        }
    }
}
