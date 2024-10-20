// src/main.rs
use axum::{
    routing::{post, get},
    extract::{Path, Query},
    Router,
    extract::Extension,
    response::{IntoResponse, Html}
};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::Duration,
    fs
};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, oneshot},
    task::{JoinHandle, spawn},
    time::sleep,
};

// Define shared state to keep track of the current background task
struct AppState {
    current_task: Mutex<Option<JoinHandle<()>>>,
    stop_signal: Mutex<Option<oneshot::Sender<()>>>,
}

#[tokio::main]
async fn main() {
    // Initialize shared state
    let state = Arc::new(AppState {
        current_task: Mutex::new(None),
        stop_signal: Mutex::new(None),
    });

    // Build the application with routes
    let app = Router::new()
        .route("/", get(home))
        .route("/move/:direction", post(move_direction))
        .route("/start_task2", post(start_task2))
        .route("/stop", post(stop_current_task))
        .layer(Extension(state));

    // Define the address for the server to bind to
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    // Run the server
    println!("Server running at http://{}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn home() -> impl IntoResponse {
    // Read the HTML file
    let html_content = fs::read_to_string("index.html")
        .unwrap_or_else(|_| "<h1>Error reading file</h1>".to_string());

    // Return the HTML content as a response
    Html(html_content)
}

// Handler to start task 1
async fn move_direction(
    Path(direction): Path<String>,
    Extension(state): Extension<Arc<AppState>>,
) -> impl IntoResponse {
    // Stop any existing task before starting a new one
    stop_current_task_helper(&state).await;

    // Create a new stop signal
    let (tx, rx) = oneshot::channel();

    // Spawn a new background task
    let handle = spawn(async move {
        let mut stop_rx = rx;

        // Set up UDP socket to send messages
        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        let target_addr = "127.0.0.1:12345".parse::<SocketAddr>().unwrap();

        if direction == "x" || direction == "y" {
            let _ = socket.send_to(direction.as_bytes(), &target_addr).await;
        } else {
            loop {
                // Task 1 specific behavior
                println!("Task 1 is running...");
                // Send a UDP message with character 'f'
                let _ = socket.send_to(direction.as_bytes(), &target_addr).await;

                // Sleep for 150 milliseconds
                sleep(Duration::from_millis(100)).await;

                // Check for stop signal
                if stop_rx.try_recv().is_ok() {
                    println!("Task 1 received stop signal.");
                    break;
                }
            }
        }
    });

    // Save the new task handle and stop signal in the state
    let mut current_task = state.current_task.lock().await;
    let mut stop_signal = state.stop_signal.lock().await;
    *current_task = Some(handle);
    *stop_signal = Some(tx);

    // Respond that the new task has started
    "Task 1 started".into_response()
}

// Handler to start task 2
async fn start_task2(
    Extension(state): Extension<Arc<AppState>>,
) -> impl IntoResponse {
    // Stop any existing task before starting a new one
    stop_current_task_helper(&state).await;

    // Create a new stop signal
    let (tx, rx) = oneshot::channel();

    // Spawn a new background task
    let handle = spawn(async move {
        let mut stop_rx = rx;
        loop {
            // Task 2 specific behavior
            println!("Task 2 is running...");
            sleep(Duration::from_secs(5)).await;

            // Check for stop signal
            if stop_rx.try_recv().is_ok() {
                println!("Task 2 received stop signal.");
                break;
            }
        }
    });

    // Save the new task handle and stop signal in the state
    let mut current_task = state.current_task.lock().await;
    let mut stop_signal = state.stop_signal.lock().await;
    *current_task = Some(handle);
    *stop_signal = Some(tx);

    // Respond that the new task has started
    "Task 2 started".into_response()
}

// Handler to stop the current task
async fn stop_current_task(
    Extension(state): Extension<Arc<AppState>>,
) -> impl IntoResponse {
    // Stop the current task if it's running
    let stopped = stop_current_task_helper(&state).await;
    if stopped {
        "Current task stopped".into_response()
    } else {
        "No task was running".into_response()
    }
}

// Helper function to stop the current task if it's running
async fn stop_current_task_helper(state: &Arc<AppState>) -> bool {
    // Lock the stop signal to notify the task
    let mut stop_signal = state.stop_signal.lock().await;
    if let Some(tx) = stop_signal.take() {
        // Send the stop signal
        let _ = tx.send(());
    }

    // Lock the current task to wait for it to finish
    let mut current_task = state.current_task.lock().await;
    if let Some(handle) = current_task.take() {
        // Await the task to finish gracefully
        let _ = handle.await;
        return true; // Task was running and has been stopped
    }
    false // No task was running
}
