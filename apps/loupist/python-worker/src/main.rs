//! One disposable WASI interpreter. It receives no host environment, native
//! Python, JavaScript runtime, socket imports, subprocess API or host callback.
//!
//! Wasmtime's compiler and the pinned CPython WASI build are the remaining
//! trusted computing base inside this process. A bug there can use this
//! process's CPU until the parent kills the process group. The 512 MiB store
//! limit bounds guest linear memory only. Compiling the pinned module uses
//! more resident memory than that cap; a warm run is closer to the cap plus
//! the runtime.
//! It does not receive the host environment, a network stack, or a preopen of
//! a live host directory. The parent owns the wall clock, the output cap, and
//! writeback.
pub mod protocol;
use protocol::*;
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use wasmtime::{
    Cache, CacheConfig, Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
};
use wasmtime_wasi::{
    FsPerms, WasiCtxBuilder,
    p1::{self, WasiP1Ctx},
};

struct State {
    wasi: WasiP1Ctx,
    limits: StoreLimits,
}

fn refuse_host_call() -> Result<i32, wasmtime::Error> {
    Err(wasmtime::Error::msg("refused"))
}

fn run(request: WorkerRequest) -> Result<i32, i32> {
    if request.code.len() > CODE_BYTES
        || request.code.contains('\0')
        || !(1..=MAX_TIMEOUT_SECONDS).contains(&request.timeout_seconds)
        || request.writable.len() > 256
        || !request.runtime.is_absolute()
        || !request.cache.is_absolute()
        || !request.snapshot.is_absolute()
        || request.writable.iter().any(|name| {
            name.is_empty()
                || name.starts_with('.')
                || name.contains(['/', '\\', '\0'])
                || name == ".."
                || name == "."
        })
    {
        return Err(125);
    }
    // Never load ambient Wasmtime configuration or a cache selected by Python.
    let mut cache = CacheConfig::new();
    cache.with_directory(&request.cache);
    let mut config = Config::new();
    config.cache(Some(Cache::new(cache).map_err(|_| 1)?));
    config
        .max_wasm_stack(8 * 1024 * 1024)
        .async_stack_size(10 * 1024 * 1024)
        .epoch_interruption(true)
        .wasm_memory64(false)
        .wasm_multi_memory(false)
        .wasm_component_model(false);
    let engine = Engine::new(&config).map_err(|_| 1)?;
    let module =
        Module::from_file(&engine, request.runtime.join("python-snapshot.wasm")).map_err(|_| 1)?;
    let mut wasi = WasiCtxBuilder::new();
    // -I suppresses environment and project startup. The submitted source is
    // compiled separately so a leading future-import still works.
    let source = format!(
        "import os\nos.chdir('/workspace')\nexec(compile({}, '<run_python>', 'exec'), {{'__name__':'__main__'}})",
        serde_json::to_string(&request.code).map_err(|_| 125)?
    );
    // Read-only preopens are the runtime and the snapshot roots. Writable
    // preopens are only child directories of the private snapshot, never a
    // live host directory. WASI resolves the longest guest prefix, so a write
    // through `/workspace` stays on the read-only preopen.
    wasi.inherit_stdout()
        .inherit_stderr()
        .args(&["/usr/local/bin/python.wasm", "-I", "-c", &source])
        .env("HOME", "/scratch")
        .env("TMPDIR", "/scratch")
        .env("MPLCONFIGDIR", "/scratch/matplotlib")
        .env("MPLBACKEND", "Agg")
        .env("PYTHONTZPATH", "")
        .env("OPENBLAS_NUM_THREADS", "1")
        .allow_ip_name_lookup(false)
        .allow_tcp(false)
        .allow_udp(false)
        .socket_addr_check(|_, _| Box::pin(async { false }));
    wasi.preopened_dir(
        request.runtime.join("lib"),
        "/usr/local/lib",
        FsPerms::ReadOnly,
    )
    .map_err(|_| 1)?;
    wasi.preopened_dir(
        request.runtime.join("mpl-data"),
        "/usr/local/lib/python315.zip/matplotlib/mpl-data",
        FsPerms::ReadOnly,
    )
    .map_err(|_| 1)?;
    wasi.preopened_dir(
        request.snapshot.join("workspace"),
        "/workspace",
        FsPerms::ReadOnly,
    )
    .map_err(|_| 1)?;
    wasi.preopened_dir(
        request.snapshot.join("attachments"),
        "/attachments",
        FsPerms::ReadOnly,
    )
    .map_err(|_| 1)?;
    wasi.preopened_dir(
        request.snapshot.join("scratch"),
        "/scratch",
        FsPerms::ReadWrite,
    )
    .map_err(|_| 1)?;
    for name in &request.writable {
        wasi.preopened_dir(
            request.snapshot.join("workspace").join(name),
            format!("/workspace/{name}"),
            FsPerms::ReadWrite,
        )
        .map_err(|_| 1)?;
    }
    let mut store = Store::new(
        &engine,
        State {
            wasi: wasi.build_p1(),
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_BYTES)
                .instances(1)
                .memories(1)
                .tables(8)
                .table_elements(1_000_000)
                .build(),
        },
    );
    store.limiter(|state| &mut state.limits);
    store.set_epoch_deadline(1);
    let mut linker = Linker::new(&engine);
    p1::add_to_linker_sync(&mut linker, |state: &mut State| &mut state.wasi).map_err(|_| 1)?;
    // The artifact imports a generic host door. Trapping it leaves no host tool
    // behind the import, whatever arguments the guest builds.
    linker
        .func_wrap("host", "call", |_a: i32, _b: i32, _c: i32, _d: i32| {
            refuse_host_call()
        })
        .map_err(|_| 1)?;
    let instance = linker.instantiate(&mut store, &module).map_err(|_| 1)?;
    let timer_engine = engine.clone();
    let finished = Arc::new(AtomicBool::new(false));
    let timer_finished = finished.clone();
    let timeout = request.timeout_seconds;
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(timeout);
        while !timer_finished.load(Ordering::Relaxed) {
            if std::time::Instant::now() >= deadline {
                timer_engine.increment_epoch();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let result = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .map_err(|_| 1)?
        .call(&mut store, ());
    finished.store(true, Ordering::Relaxed);
    match result {
        Ok(status) => Ok(status),
        Err(error) => {
            // Debug text can name a host path. Classify it, then print a fixed line.
            let detail = format!("{error:?}").to_ascii_lowercase();
            if detail.contains("epoch") || detail.contains("interrupt") {
                eprintln!("Python execution timed out");
                Err(124)
            } else if detail.contains("memory")
                || detail.contains("limit")
                || detail.contains("trap")
                || detail.contains("refused")
            {
                eprintln!("Python execution exceeded a resource limit");
                Err(126)
            } else {
                eprintln!("Python execution stopped");
                Err(1)
            }
        }
    }
}

fn drop_inherited_environment() {
    let names = std::env::vars().map(|(key, _)| key).collect::<Vec<_>>();
    for name in names {
        unsafe { std::env::remove_var(name) };
    }
}

fn arm_parent_death() {
    #[cfg(target_os = "linux")]
    unsafe {
        let parent = libc::getppid();
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
        if libc::getppid() != parent {
            libc::_exit(125);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let parent = unsafe { libc::getppid() };
        std::thread::spawn(move || unsafe {
            let queue = libc::kqueue();
            if queue < 0 {
                libc::_exit(125);
            }
            let mut change: libc::kevent = std::mem::zeroed();
            change.ident = parent as libc::uintptr_t;
            change.filter = libc::EVFILT_PROC;
            change.flags = libc::EV_ADD | libc::EV_ENABLE;
            change.fflags = libc::NOTE_EXIT;
            if libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) < 0
                || libc::getppid() != parent
            {
                libc::_exit(125);
            }
            let mut event: libc::kevent = std::mem::zeroed();
            libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, std::ptr::null());
            libc::_exit(125);
        });
    }
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
    {
        let parent = unsafe { libc::getppid() };
        std::thread::spawn(move || {
            loop {
                if unsafe { libc::getppid() } != parent {
                    unsafe { libc::_exit(125) };
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
    }
}

fn main() {
    arm_parent_death();
    drop_inherited_environment();
    let mut bytes = Vec::new();
    if std::io::stdin()
        .take(MAX_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > MAX_REQUEST_BYTES
    {
        std::process::exit(125);
    }
    let request = match serde_json::from_slice::<WorkerRequest>(&bytes) {
        Ok(value) => value,
        Err(_) => std::process::exit(125),
    };
    let result = std::thread::Builder::new()
        .name("python-wasi".into())
        .stack_size(12 * 1024 * 1024)
        .spawn(move || run(request))
        .expect("Python worker thread")
        .join();
    let status = match result {
        Ok(Ok(status)) if (0..=255).contains(&status) => status,
        Ok(Ok(_)) => 1,
        Ok(Err(status)) => status,
        Err(_) => {
            eprintln!("Python worker stopped");
            1
        }
    };
    std::process::exit(status);
}
