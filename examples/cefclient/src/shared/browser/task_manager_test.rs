use super::test_runner::*;
use cef::{wrapper::message_router::*, *};
use serde::Serialize;
use std::sync::{Arc, Mutex};

const TEST_URL_PATH: &str = "/task_manager";

#[derive(Serialize)]
struct JsonTaskInfo {
    id: i64,
    #[serde(rename = "type")]
    task_type: String,
    is_killable: bool,
    title: String,
    cpu_usage: f64,
    number_of_processors: i32,
    memory: i64,
    gpu_memory: i64,
    is_gpu_memory_inflated: bool,
    is_this_browser: bool,
}

impl JsonTaskInfo {
    fn new(value: &TaskInfo, browser_task_id: Option<i64>) -> Self {
        Self {
            id: value.id,
            task_type: task_type_to_string(value.type_).to_string(),
            is_killable: value.is_killable != 0,
            title: value.title.to_string(),
            cpu_usage: value.cpu_usage,
            number_of_processors: value.number_of_processors,
            memory: value.memory,
            gpu_memory: value.gpu_memory,
            is_gpu_memory_inflated: value.is_gpu_memory_inflated != 0,
            is_this_browser: browser_task_id == Some(value.id),
        }
    }
}

#[derive(Serialize)]
struct TaskList {}

fn task_type_to_string(task_type: TaskType) -> &'static str {
    match task_type {
        TaskType::BROWSER => "Browser",
        TaskType::GPU => "GPU",
        TaskType::ZYGOTE => "Zygote",
        TaskType::UTILITY => "Utility",
        TaskType::RENDERER => "Renderer",
        TaskType::EXTENSION => "Extension",
        TaskType::GUEST => "Guest",
        TaskType::PLUGIN_DEPRECATED => "Plugin (Deprecated)",
        TaskType::SANDBOX_HELPER => "Sandbox Helper",
        TaskType::DEDICATED_WORKER => "Dedicated Worker",
        TaskType::SHARED_WORKER => "Shared Worker",
        TaskType::SERVICE_WORKER => "Service Worker",
        TaskType::NUM_VALUES => unreachable!("sentinel value"),
        _ => "Unknown",
    }
}

struct Handler {
    task_manager: Option<TaskManager>,
}

impl Handler {
    fn new() -> Self {
        Handler {
            task_manager: task_manager_get(),
        }
    }
}

impl BrowserSideHandler for Handler {
    /// Called due to cefQuery execution in task_manager.html.
    fn on_query_str(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        _query_id: i64,
        request: &str,
        _persistent: bool,
        callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) -> bool {
        // Only handle messages from the test URL.
        if !is_test_url(frame, TEST_URL_PATH) {
            return false;
        }

        if let (Some(task_manager), Ok(callback)) = (self.task_manager.as_ref(), callback.lock()) {
            if request == "get_tasks" {
                let count = task_manager.tasks_count();
                let mut task_ids = vec![0; count];
                if task_manager.task_ids_list(Some(&mut task_ids)) != 0 {
                    let browser_task_id = browser
                        .map(|browser| task_manager.task_id_for_browser_id(browser.identifier()));
                    let tasks: Vec<_> = task_ids
                        .into_iter()
                        .filter_map(|task_id| {
                            let mut info = Default::default();
                            if task_manager.task_info(task_id, Some(&mut info)) != 0 {
                                Some(JsonTaskInfo::new(&info, browser_task_id))
                            } else {
                                None
                            }
                        })
                        .collect();

                    if let Ok(message) = serde_json::to_string(&tasks) {
                        callback.success_str(message.as_str());
                    }
                }
            } else if let Ok(task_id) = request.parse() {
                task_manager.kill_task(task_id);
                callback.success_str("");
            }
        }

        true
    }
}

pub fn create_message_handler() -> Arc<dyn BrowserSideHandler> {
    Arc::new(Handler::new())
}
