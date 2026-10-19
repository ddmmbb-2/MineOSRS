mod downloads;
mod legacy_paper;
mod bedrock;
mod hardware;
mod backups;

use std::{
    collections::{HashMap, VecDeque},
    env, fs,
    net::{SocketAddr, TcpListener as StdTcpListener, UdpSocket as StdUdpSocket},
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path as AxPath, Query, State},
    http::{header, HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::{params, Connection};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::password_hash::SaltString;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, RwLock},
    time,
};
use tracing::{info, warn};
use uuid::Uuid;

const MAX_LOG_LINES: usize = 1200;
const MAX_UPLOAD_BYTES: usize = 512 * 1024 * 1024;

type ApiResult<T> = Result<T, (StatusCode, String)>;


// AI-first domain slices: compiled into this crate without API/type-adapter layers.
// The include order preserves the original item scope and public behavior.
include!("core/state.rs");
include!("core/bootstrap.rs");
include!("core/auth.rs");
include!("core/assets.rs");
include!("core/validation.rs");
include!("core/servers.rs");
include!("core/process.rs");
include!("core/uploads.rs");
include!("core/errors.rs");
