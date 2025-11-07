// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::Datelike;
use chrono::{Local, NaiveDate};
use directories::ProjectDirs;
use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use serde::{Deserialize, Serialize};
use slint::Timer;
use slint::{LogicalPosition, SharedString};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

slint::include_modules!();

const SETTINGS_FILE: &str = "config.json";

#[derive(Serialize, Deserialize, Clone)]
struct AppSettings {
    title: String,
    exam_date: String, // 格式: YYYY-MM-DD
    auto_start: bool,
    window_x: f32,
    window_y: f32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            title: "距离截止日期还有".to_string(),
            exam_date: Local::now().format("%Y-%m-%d").to_string(),
            auto_start: false,
            window_x: 100.0,
            window_y: 100.0,
        }
    }
}

fn get_settings_path() -> Option<PathBuf> {
    if let Some(proj_dirs) = ProjectDirs::from_path("ExamCountdown".parse().unwrap()) {
        let config_dir = proj_dirs.config_dir();
        fs::create_dir_all(config_dir).ok()?;
        Some(config_dir.join(SETTINGS_FILE))
    } else {
        // 备用方案：使用当前目录
        Some(PathBuf::from(SETTINGS_FILE))
    }
}

fn load_settings() -> AppSettings {
    if let Some(settings_path) = get_settings_path() {
        if let Ok(contents) = fs::read_to_string(&settings_path) {
            if let Ok(settings) = serde_json::from_str(&contents) {
                return settings;
            }
        }
    }
    AppSettings::default()
}

fn save_settings(settings: &AppSettings) -> Result<(), Box<dyn Error>> {
    if let Some(settings_path) = get_settings_path() {
        let contents = serde_json::to_string_pretty(settings)?;
        fs::write(settings_path, contents)?;
    }
    Ok(())
}

fn parse_date_fallback(date_str: &str) -> Option<(BigInt, u32, u32)> {
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return None;
    }

    let year = parts[0].parse::<BigInt>().ok()?;
    let month = parts[1].parse::<u32>().ok()?;
    let day = parts[2].parse::<u32>().ok()?;

    if month < 1 || month > 12 || day < 1 || day > 31 {
        return None;
    }

    Some((year, month, day))
}

fn calculate_days_until(target_date: &str) -> (BigInt, bool) {
    let today = Local::now().naive_local().date();

    // 首先尝试使用 chrono 解析日期
    if let Ok(target) = NaiveDate::parse_from_str(target_date, "%Y-%m-%d") {
        let duration = target - today;
        let days = BigInt::from(duration.num_days());
        if days.is_negative() {
            (-days, true) // 返回过期天数和过期标志
        } else {
            (days, false) // 返回剩余天数和未过期标志
        }
    } else {
        // 如果 chrono 无法解析，则使用自定义解析方法
        if let Some((target_year, target_month, target_day)) = parse_date_fallback(target_date) {
            let today_year = BigInt::from(today.year());
            let today_month = today.month();
            let today_day = today.day();

            let year_diff = &target_year - &today_year;

            let total_days = if year_diff.is_zero() {
                // 同一年内的计算
                let month_diff = BigInt::from(target_month as i32 - today_month as i32);
                let day_diff = BigInt::from(target_day as i32 - today_day as i32);
                &month_diff * BigInt::from(30) + &day_diff
            } else {
                // 跨年度计算
                let base_days = &year_diff * BigInt::from(365); // 基本年数天数
                let leap_days = &year_diff / BigInt::from(4); // 闰年天数（简化计算）
                let month_diff = BigInt::from(target_month as i32 - today_month as i32);
                let day_diff = BigInt::from(target_day as i32 - today_day as i32);
                let additional_days = &month_diff * BigInt::from(30) + &day_diff;
                base_days + leap_days + additional_days
            };

            if total_days.is_negative() {
                (-total_days, true)
            } else {
                (total_days, false)
            }
        } else {
            (BigInt::zero(), false) // 默认返回0天未过期
        }
    }
}

fn setup_auto_start(enable: bool) -> Result<(), Box<dyn Error>> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = r"Software\Microsoft\Windows\CurrentVersion\Run";

        if let Ok(run) = hkcu.open_subkey_with_flags(path, KEY_WRITE) {
            let app_name = "DeadlineCount";
            let exe_path = std::env::current_exe()?.to_string_lossy().to_string();

            if enable {
                run.set_value(app_name, &exe_path)?;
            } else {
                let _ = run.delete_value(app_name);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // TODO：支持 MACOS 添加开机自启动
    }

    #[cfg(target_os = "linux")]
    {
        // TODO：支持 Linux 添加开机自启动
    }

    Ok(())
}

fn update_countdown_display(ui: &AppWindow, settings: &AppSettings) {
    let (days, is_expired) = calculate_days_until(&settings.exam_date);
    // 将 BigInt 转换为字符串后再格式化
    let days_str = format!("{}", days);
    let formatted_days = if days_str.len() > 3 {
        // 千位分隔符格式化
        let mut result = String::new();
        let chars: Vec<char> = days_str.chars().collect();
        let len = chars.len();

        for (i, ch) in chars.iter().enumerate() {
            result.push(*ch);
            // 每三位数字添加一个逗号（从右往左数）
            if (len - i - 1) % 3 == 0 && i != len - 1 {
                result.push(',');
            }
        }
        result
    } else {
        days_str
    };

    let display_text = if is_expired {
        format!(
            "{} 0 天（倒计时已过期{}天）",
            settings.title, formatted_days
        )
    } else {
        format!("{} {} 天", settings.title, formatted_days)
    };
    ui.set_countdown_text(display_text.into());
}

fn main() -> Result<(), Box<dyn Error>> {
    let settings = load_settings();

    let ui = AppWindow::new()?;
    let handle = ui.as_weak();

    // 设置窗口初始位置
    ui.window()
        .set_position(LogicalPosition::new(settings.window_x, settings.window_y));

    // 更新倒计时显示
    update_countdown_display(&ui, &settings);

    // 每分钟更新一次倒计时
    let timer_handle = handle.clone();
    let timer = Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(60),
        move || {
            if let Some(main) = timer_handle.upgrade() {
                let current_settings = load_settings();
                update_countdown_display(&main, &current_settings);
            }
        },
    );

    let handle_close = handle.clone();
    ui.on_close_window(move || {
        if let Some(main) = handle_close.upgrade() {
            // 保存窗口位置
            let window = main.window();
            let pos = window.position().to_logical(window.scale_factor());
            let mut current_settings = load_settings();
            current_settings.window_x = pos.x;
            current_settings.window_y = pos.y;
            let _ = save_settings(&current_settings);
            let _ = main.hide();
        }
    });

    let handle_move = handle.clone();
    ui.on_move_window(move |offset_x, offset_y| {
        if let Some(main) = handle_move.upgrade() {
            let window = main.window();
            let logical_pos = window.position().to_logical(window.scale_factor());
            window.set_position(LogicalPosition::new(
                logical_pos.x + offset_x,
                logical_pos.y + offset_y,
            ));
        }
    });

    let handle_save = handle.clone();
    ui.on_save_settings(
        move |title: SharedString, exam_date: SharedString, auto_start| {
            if let Some(main) = handle_save.upgrade() {
                // 更新倒计时显示
                let (days, is_expired) = calculate_days_until(&exam_date);
                // 将 BigInt 转换为字符串后再格式化
                let days_str = format!("{}", days);
                let formatted_days = if days_str.len() > 3 {
                    // 千位分隔符格式化
                    let mut result = String::new();
                    let chars: Vec<char> = days_str.chars().collect();
                    let len = chars.len();

                    for (i, ch) in chars.iter().enumerate() {
                        result.push(*ch);
                        // 每三位数字添加一个逗号（从右往左数）
                        if (len - i - 1) % 3 == 0 && i != len - 1 {
                            result.push(',');
                        }
                    }
                    result
                } else {
                    days_str
                };

                let display_text = if is_expired {
                    format!("{} 0 天（倒计时已过期{}天）", title, formatted_days)
                } else {
                    format!("{} {} 天", title, formatted_days)
                };
                main.set_countdown_text(display_text.into());

                // 保存设置
                let window = main.window();
                let pos = window.position().to_logical(window.scale_factor());

                let new_settings = AppSettings {
                    title: title.to_string(),
                    exam_date: exam_date.to_string(),
                    auto_start,
                    window_x: pos.x,
                    window_y: pos.y,
                };

                if let Err(e) = save_settings(&new_settings) {
                    eprintln!("保存设置失败: {}", e);
                }

                // 设置开机自启动
                if let Err(e) = setup_auto_start(auto_start) {
                    eprintln!("设置开机自启动失败: {}", e);
                }
            }
        },
    );

    let handle_open = handle.clone();
    ui.on_open_settings(move || {
        if let Some(_main) = handle_open.upgrade() {
            let current_settings = load_settings();

            // 创建设置窗口
            let settings_window = SettingsWindow::new().unwrap();
            let _settings_handle = settings_window.as_weak();

            // 设置初始值
            settings_window.set_input_title(current_settings.title.into());
            settings_window.set_input_date(current_settings.exam_date.into());
            settings_window.set_input_auto_start(current_settings.auto_start);

            let handle_save_settings = handle.clone();
            let settings_weak = settings_window.as_weak();
            settings_window.on_saved(
                move |title: SharedString, exam_date: SharedString, auto_start| {
                    if let Some(main) = handle_save_settings.upgrade() {
                        // 触发主窗口的保存设置回调
                        main.invoke_save_settings(title, exam_date, auto_start);
                    }
                    // 保存完成后隐藏设置窗口
                    if let Some(window) = settings_weak.upgrade() {
                        window.hide().unwrap();
                    }
                },
            );

            let handle_close_settings = _settings_handle.clone();
            settings_window.on_window_closed(move || {
                if let Some(settings) = handle_close_settings.upgrade() {
                    settings.hide().unwrap();
                }
            });

            // 显示设置窗口
            settings_window.show().unwrap();
        }
    });

    ui.run()?;

    Ok(())
}
