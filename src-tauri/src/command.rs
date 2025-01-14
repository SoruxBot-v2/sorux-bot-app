use std::{
    fs::{self, File},
    io::Read,
    process::Child,
    sync::Arc,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Number;
use tauri::Window;
use walkdir::WalkDir;

use crate::{
    global_channels::{
        CHILD_PROCESS_MAP, PROVIDER_BOT_LOGIN_CHANNEL, WRAPPER_LOGS_CHANNEL,
    },
    global_constants::{
        PLUGIN_BIN_DIR, PLUGIN_CONF_DIR, PROVIDER_CHILD_NAME, PROVIDER_DIR_PATH, WRAPPER_CHILD_NAME,
    },
    provider, wrapper,
};

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn upload_file(file: Vec<u8>, filename: &str) -> Result<String, String> {
    use std::fs::File;
    use std::io::Write;

    let path = match filename.ends_with(".dll") {
        true => format!("{}/{}", PLUGIN_BIN_DIR, filename),
        false => format!("{}/{}", PLUGIN_CONF_DIR, filename),
    };

    let mut output = File::create(path).map_err(|e| e.to_string())?;
    output.write_all(&file).map_err(|e| e.to_string())?;

    Ok("插件添加成功！".to_string())
}

#[tauri::command]
pub fn upload_plugin(
    json_file: Vec<u8>,
    dll_file: Vec<u8>,
    json_filename: String,
) -> Result<String, String> {
    let mut res = String::new();
    let mut is_err = false;
    restart_child_process(vec![WRAPPER_CHILD_NAME], || {
        if let Err(e) = upload_file(json_file, &json_filename) {
            is_err = true;
            res = e;
            return;
        }

        if let Err(e) = upload_file(dll_file, &json_filename.replace(".json", ".dll")) {
            is_err = true;
            res = e;
            return;
        }

        res = String::from("插件添加成功！");
    });

    if is_err {
        Err(res)
    } else {
        Ok(res)
    }
    // 同名上传
    // upload_file(json_file, &json_filename)
    //     .and_then(|_| upload_file(dll_file, &json_filename.replace(".json", ".dll")))
    //     .and_then(|_| {
    //         // and_then: Calls op if the result is Ok, otherwise returns the Err value of self.

    //         Ok(String::from("插件添加成功！"))
    //     })
}

#[derive(Serialize, Deserialize)]
pub struct PluginItem {
    name: String,
    privilege: Number,
    filename: Option<String>, // 记录文件原始路径
}

#[tauri::command]
pub fn get_plugins() -> Result<Vec<PluginItem>, String> {
    let mut res: Vec<PluginItem> = Vec::new(); // 创建一个存放名字的向量
    let path = format!("{}/", PLUGIN_CONF_DIR); // 指定要遍历的目录

    // 遍历目录和子目录中的所有文件
    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        // 检查文件是否是 JSON 文件
        if path.extension().and_then(std::ffi::OsStr::to_str) == Some("json") {
            // 读取文件内容
            let data = fs::read_to_string(path).map_err(|e| e.to_string()).unwrap();
            println!("{}", data);
            // 解析 JSON 数据
            let mut item: PluginItem = serde_json::from_str(&data.to_lowercase())
                .map_err(|e| e.to_string())
                .unwrap();

            println!(
                "{}",
                path.file_name().and_then(std::ffi::OsStr::to_str).unwrap()
            );
            item.filename = Some(
                path.file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap()
                    .to_string(),
            );
            res.push(item);
        }
    }

    Ok(res)
}

#[tauri::command]
pub fn del_plugins(filename: String) -> String {
    let mut res = String::new();
    restart_child_process(vec![WRAPPER_CHILD_NAME], || {
        if let Err(e) = fs::remove_file(format!("{}/{}", PLUGIN_CONF_DIR, filename)) {
            res = String::from(e.to_string());
            return;
        }

        res = if let Err(e) = fs::remove_file(format!(
            "{}/{}",
            PLUGIN_BIN_DIR,
            filename.replace(".json", ".dll")
        )) {
            String::from(e.to_string())
        } else {
            String::new()
        };
    });

    res
}

#[tauri::command]
pub fn watch_qrcode(window: Window) {
    std::thread::spawn(move || loop {
        // 每三秒获取一次二维码推送给前端 (1+2)
        std::thread::sleep(std::time::Duration::from_secs(1));
        window
            .emit(
                "qrcode-event",
                Payload {
                    message: get_qrcode().unwrap_or_default(),
                },
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
    });
}
// #[tauri::command]
fn get_qrcode() -> Result<String, String> {
    let mut file = File::open(format!("{}/qr.png", PROVIDER_DIR_PATH))
        .map_err(|e| e.to_string())
        .unwrap();

    // 读取文件内容到一个 vector
    let mut buffer = Vec::new();
    if let Err(e) = file.read_to_end(&mut buffer) {
        println!("{}", e.to_string());
        Err(e.to_string())
    } else {
        // 将数据编码为 Base64 并返回
        Ok(STANDARD.encode(buffer))
    }
}

// 负载类型必须实现 `Serialize` 和 `Clone`。
#[derive(Clone, serde::Serialize)]
struct Payload {
    message: String,
}

// 在 command 中初始化后台进程，并仅向使用该命令的窗口发出周期性事件
#[tauri::command]
pub fn init_process(window: Window) {
    std::thread::spawn(move || {
        let receiver = Arc::clone(&PROVIDER_BOT_LOGIN_CHANNEL.1);
        let receiver = receiver.lock().unwrap();

        // 这里前端收到消息就说明登陆成功了
        for _ in receiver.iter() {
            window
                .emit(
                    "my-event",
                    Payload {
                        message: "Tauri is awesome!".into(),
                    },
                )
                .unwrap();
        }
    });
}

fn restart_child_process<F>(child_process_names: Vec<&str>, call_back: F)
where
    F: FnOnce(),
{
    let mut child_process: Child;
    {
        // 每次拿到锁，释放锁后一定还是2个子进程
        let mut map = CHILD_PROCESS_MAP.lock().unwrap();
        for ele in child_process_names.clone() {
            child_process = map.remove(ele).unwrap();
            // 结束子进程
            let _ = child_process.kill().expect("Failed to kill child process");
            // 等待子进程结束
            let _ = child_process.wait().expect("Failed to wait on child");
        }

        println!(
            "exit child process {:?} and ready to restart...",
            child_process_names
        );

        // 执行回调
        call_back();
        println!("执行回调中");
        // 启动新的子进程

        for ele in child_process_names {
            match ele {
                PROVIDER_CHILD_NAME => {
                    child_process = provider::run_provider();
                    map.insert(PROVIDER_CHILD_NAME.to_string(), child_process);
                }
                WRAPPER_CHILD_NAME => {
                    child_process = wrapper::run_wrapper();
                    map.insert(WRAPPER_CHILD_NAME.to_string(), child_process);
                }
                _ => {
                    panic!("未知进程名，出错了！")
                }
            }
        }

        println!("restart child process successfully...");
    }
}

// 登出
#[tauri::command]
pub fn logout() {
    restart_child_process(vec![PROVIDER_CHILD_NAME, WRAPPER_CHILD_NAME], || {});
}

// provider的日志
#[tauri::command]
pub fn wrapper_logs(window: Window) {
    std::thread::spawn(move || {
        let receiver = Arc::clone(&WRAPPER_LOGS_CHANNEL.1);
        let receiver = receiver.lock().unwrap();
        println!("等待接收");
        // 日志不断推送给前端
        for line in receiver.iter() {
            println!("to front: {}", line);
            window
                .emit("wrapper-logs-event", Payload { message: line })
                .unwrap();
        }
    });
}
