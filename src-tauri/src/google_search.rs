//! Experimental Google AI Mode browser. Remote pages receive no Tauri capability.
use tauri::menu::{Menu, MenuItem};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

const LABEL: &str = "google-ai-browser";

const EMBED: &str = "google-ai-embedded";
static PANEL_DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const BACKGROUND_SCRIPT: &str = r#"(() => {
 if (location.hostname !== 'www.google.com') return;
 const dark = __DARK__;
 window.__dockPanelDark = dark;
 const guardCss = () => `html{background-color:${window.__dockPanelDark ? '#202028' : '#eeedf2'}!important}${window.__dockPanelReady ? '' : 'body{visibility:hidden!important}'}`;
 const installGuard = () => {
  if (!document.documentElement) return false;
  let style = document.getElementById('dock-panel-first-paint');
  if (!style) { style = document.createElement('style'); style.id = 'dock-panel-first-paint'; document.documentElement.appendChild(style); }
  style.textContent = guardCss();
  return true;
 };
 if (!installGuard()) {
  const early = new MutationObserver(() => { if (installGuard()) early.disconnect(); });
  early.observe(document, {childList:true});
 }
 if (!window.__dockPanelColors) window.__dockPanelColors = new WeakMap();
 const paint = () => {
  const dark = window.__dockPanelDark;
  const bg = dark ? '#202028' : '#eeedf2';
  const fg = dark ? '#eeeeF2' : '#202026';
  const neutral = value => {
   const n = value.match(/[\d.]+/g)?.map(Number);
   return n && n.length >= 3 && (n.length < 4 || n[3] > .9) && Math.max(...n.slice(0,3)) - Math.min(...n.slice(0,3)) < 32;
  };
  document.documentElement.style.setProperty('background-color', bg, 'important');
  document.body?.style.setProperty('background-color', bg, 'important');
  for (const el of document.querySelectorAll('body,main,nav,aside,header,section,div,span,p,h1,h2,h3,h4,li,td,th,button,input,textarea,a,svg')) {
   let original = window.__dockPanelColors.get(el);
   if (!original) { const s = getComputedStyle(el); original = {bg:s.backgroundColor,fg:s.color}; window.__dockPanelColors.set(el, original); }
   if (neutral(original.bg)) el.style.setProperty('background-color', bg, 'important');
   if (neutral(original.fg)) el.style.setProperty('color', fg, 'important');
  }
  let controls = document.getElementById('dock-panel-google-controls');
  if (!controls) {
   controls = document.createElement('style'); controls.id = 'dock-panel-google-controls';
   controls.textContent = '[data-dock-share],[data-dock-share] *{background:transparent!important;box-shadow:none!important}[data-dock-share]:hover{background:var(--dock-google-hover)!important}[data-dock-input-wrapper]::before,[data-dock-input-wrapper]::after{background:transparent!important;background-image:none!important}';
   document.head.appendChild(controls);
  }
  document.documentElement.style.setProperty('--dock-google-hover', dark ? 'rgba(255,255,255,.08)' : 'rgba(0,0,0,.06)');
  for (const el of document.querySelectorAll('button,[role="button"]')) {
   const label = (el.getAttribute('aria-label') || el.getAttribute('title') || '').trim();
   if (/^(поделиться|share)(\b|\s|$)/i.test(label)) el.setAttribute('data-dock-share','');
  }
  for (const input of document.querySelectorAll('textarea,[contenteditable="true"],[role="textbox"],span,div')) {
   const label = [input.getAttribute('placeholder'),input.getAttribute('aria-label'),input.getAttribute('data-placeholder'),input.children.length === 0 ? input.textContent.trim() : ''].filter(Boolean).join(' ');
   if (!/задайте вопрос|ask anything|ask a question/i.test(label)) continue;
   let card = null;
   let el = input.parentElement;
   for (let depth = 0; el && depth < 10; depth++,el = el.parentElement) {
    const s = getComputedStyle(el); const r = el.getBoundingClientRect();
    if (!card && parseFloat(s.borderRadius) >= 12 && r.width >= 250 && r.height >= 60 && r.height <= 250) {
     card = el;
     el.style.setProperty('background-color', dark ? '#2b2b35' : '#ffffff', 'important');
    } else if (card) {
     el.setAttribute('data-dock-input-wrapper','');
     el.style.setProperty('background-color', bg, 'important');
     el.style.setProperty('background-image', 'none', 'important');
     for (const layer of el.children) {
      if (layer === card || layer.contains(input) || layer.contains(card)) continue;
      const ls = getComputedStyle(layer);
      if (ls.backgroundImage.includes('gradient') && !layer.innerText.trim()) {
       layer.style.setProperty('background', 'transparent', 'important');
      }
     }
    }
   }
  }
 };
 window.__dockPanelPaint = paint;
 const reveal = () => {
  try { paint(); } finally {
   window.__dockPanelReady = true;
   installGuard();
  }
 };
 if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', () => requestAnimationFrame(reveal), {once:true});
 } else reveal();
 // A broken/slow page must never remain invisible indefinitely.
 if (!window.__dockPanelRevealTimer) window.__dockPanelRevealTimer = setTimeout(() => {
  if (!window.__dockPanelReady) reveal();
 }, 5000);
 if (!window.__dockPanelObserver) {
   let frame;
  const observe = () => {
   window.__dockPanelObserver = new MutationObserver(() => {
    if (frame) return;
    frame = requestAnimationFrame(() => { frame = null; window.__dockPanelPaint(); });
   });
   window.__dockPanelObserver.observe(document.body, {childList:true,subtree:true});
  };
  if (document.body) observe(); else document.addEventListener('DOMContentLoaded', observe, {once:true});
 }
})()"#;

fn background_script(dark: bool) -> String {
    BACKGROUND_SCRIPT.replace("__DARK__", if dark { "true" } else { "false" })
}

fn browser_background(dark: bool) -> tauri::webview::Color {
    if dark {
        tauri::webview::Color(32, 32, 40, 255)
    } else {
        tauri::webview::Color(238, 237, 242, 255)
    }
}

#[tauri::command]
pub async fn google_ai_theme(app: AppHandle, dark: bool) -> Result<(), String> {
    PANEL_DARK.store(dark, std::sync::atomic::Ordering::SeqCst);
    if let Some(view) = app.get_webview(EMBED) {
        view.set_background_color(Some(browser_background(dark)))
            .map_err(|e| e.to_string())?;
        view.eval(background_script(dark))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

const PANEL_STYLE: &str = r#"(() => {
 if (location.hostname !== 'www.google.com') return;
 const apply = () => {
  if (document.getElementById('dock-panel-scrollbar')) return;
  const style = document.createElement('style'); style.id = 'dock-panel-scrollbar';
  style.textContent = '*{scrollbar-width:thin!important;scrollbar-color:rgba(128,128,128,.45) transparent!important}::-webkit-scrollbar{width:6px;height:6px}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-thumb{background:rgba(128,128,128,.45);border-radius:8px}::-webkit-scrollbar-thumb:hover{background:rgba(128,128,128,.65)}';
  (document.head || document.documentElement).appendChild(style);
 };
 if (document.documentElement) apply(); else document.addEventListener('DOMContentLoaded', apply, {once:true});
})()"#;

#[tauri::command]
pub async fn google_ai_zoom(app: AppHandle, zoom: f64) -> Result<(), String> {
    if !zoom.is_finite() || !(0.5..=2.0).contains(&zoom) {
        return Err("Масштаб должен быть от 50 до 200%".into());
    }
    app.get_webview(EMBED)
        .ok_or("Браузер не открыт")?
        .set_zoom(zoom)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct GoogleSelection {
    pub text: String,
    pub url: String,
}

#[tauri::command]
pub async fn google_ai_selection(app: AppHandle) -> Result<GoogleSelection, String> {
    let view = app.get_webview(EMBED).ok_or("Браузер не открыт")?;
    if view.url().map_err(|e| e.to_string())?.host_str() != Some("www.google.com") {
        return Err("Выделите текст на странице Google".into());
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = std::sync::Mutex::new(Some(tx));
    view.eval_with_callback(
        "({text: window.getSelection()?.toString().slice(0,50000) || '', url: location.href})",
        move |value| {
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(value);
            }
        },
    )
    .map_err(|e| e.to_string())?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), rx)
        .await
        .map_err(|_| "Страница не ответила".to_string())?
        .map_err(|e| e.to_string())?;
    let selection: GoogleSelection =
        serde_json::from_str(&result).map_err(|_| "Не удалось прочитать выделение".to_string())?;
    if selection.text.trim().is_empty() {
        return Err("Сначала выделите текст в Google".into());
    }
    Ok(selection)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct GoogleChat {
    id: usize,
    title: String,
    url: Option<String>,
}

#[tauri::command]
pub async fn google_ai_history(app: AppHandle) -> Result<Vec<GoogleChat>, String> {
    let view = app.get_webview(EMBED).ok_or("Сначала откройте Google AI")?;
    if view.url().map_err(|e| e.to_string())?.host_str() != Some("www.google.com") {
        return Err("История доступна только на странице Google".into());
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    view.eval_with_callback(HISTORY_SCRIPT, move |result| {
        if let Some(sender) = sender.lock().unwrap().take() {
            let _ = sender.send(result);
        }
    })
    .map_err(|e| e.to_string())?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(8), receiver)
        .await
        .map_err(|_| "Страница не ответила. Попробуйте ещё раз".to_string())?
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&result).map_err(|_| "Не удалось прочитать список чатов".into())
}

#[tauri::command]
pub async fn google_ai_chat(app: AppHandle, id: usize) -> Result<(), String> {
    if id >= 100 {
        return Err("Некорректный чат".into());
    }
    let view = app.get_webview(EMBED).ok_or("Браузер не открыт")?;
    if view.url().map_err(|e| e.to_string())?.host_str() != Some("www.google.com") {
        return Err("Откройте Google AI".into());
    }
    view.eval(format!("(() => {{ const item = window.__dockGoogleChats?.[{id}]; if (item?.isConnected) item.click(); }})()"))
        .map_err(|e| e.to_string())
}

const HISTORY_SCRIPT: &str = r#"(() => {
  if (location.hostname !== 'www.google.com') return [];
  const visible = el => { const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
  const headings = [...document.querySelectorAll('span,div,h2,h3')].filter(el => visible(el) && el.children.length === 0 && /^(Недавнее|Recent|Recents)$/i.test(el.textContent.trim()));
  let sidebar = headings[0];
  while (sidebar) {
    const r = sidebar.getBoundingClientRect();
    if (r.width >= 150 && r.width <= 380 && r.height >= 250) break;
    sidebar = sidebar.parentElement;
  }
  if (!sidebar) sidebar = [...document.querySelectorAll('aside,nav,[role="navigation"]')].find(el => visible(el) && /Недавнее|Recent/i.test(el.innerText));
  if (!sidebar) { window.__dockGoogleChats = []; return []; }
  const heading = headings.find(el => sidebar.contains(el));
  const headingY = heading?.getBoundingClientRect().bottom ?? sidebar.getBoundingClientRect().top;
  const skip = /^(Новый чат|New chat|Поиск цепочек|Search threads|Настройки|Settings|Недавнее|Recent)$/i;
  const entries = [...sidebar.querySelectorAll('a,button,[role="button"],[role="link"]')].filter(el => {
    const text = (el.innerText || el.getAttribute('aria-label') || '').trim();
    return visible(el) && el.getBoundingClientRect().top >= headingY && text && !skip.test(text) && !el.querySelector('a,button,[role="button"],[role="link"]');
  }).slice(0,100);
  window.__dockGoogleChats = entries;
  return entries.map((el,id) => ({ id, title: (el.innerText || el.getAttribute('aria-label')).trim().slice(0,160), url: el.href && new URL(el.href).hostname === 'www.google.com' ? el.href : null }));
})()"#;
static EMBED_VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
pub async fn google_ai_visible(app: AppHandle, visible: bool) -> Result<(), String> {
    EMBED_VISIBLE.store(visible, std::sync::atomic::Ordering::SeqCst);
    if !visible {
        if let Some(view) = app.get_webview(EMBED) {
            view.hide().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn google_ai_embed(
    app: AppHandle,
    query: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    navigate: bool,
) -> Result<(), String> {
    if ![x, y, width, height].iter().all(|v| v.is_finite())
        || x < 0.0
        || y < 0.0
        || width < 1.0
        || height < 1.0
    {
        return Err("Некорректный размер области браузера".into());
    }
    let url = search_url(&query)?;
    let position = tauri::LogicalPosition::new(x, y);
    let size = tauri::LogicalSize::new(width, height);
    if let Some(view) = app.get_webview(EMBED) {
        view.set_bounds(tauri::Rect {
            position: position.into(),
            size: size.into(),
        })
        .map_err(|e| e.to_string())?;
        if navigate {
            view.navigate(url).map_err(|e| e.to_string())?;
        }
        return if EMBED_VISIBLE.load(std::sync::atomic::Ordering::SeqCst) {
            view.show()
        } else {
            view.hide()
        }
        .map_err(|e| e.to_string());
    }
    if let Some(old) = app.get_webview_window(LABEL) {
        let _ = old.close();
    }
    let profile = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("google-browser-profile");
    let main = app.get_window("main").ok_or("Панель не найдена")?;
    let view = main
        .add_child(
            tauri::webview::WebviewBuilder::new(EMBED, WebviewUrl::External(url))
                .background_color(browser_background(
                    PANEL_DARK.load(std::sync::atomic::Ordering::SeqCst),
                ))
                .initialization_script(PANEL_STYLE)
                .initialization_script(background_script(
                    PANEL_DARK.load(std::sync::atomic::Ordering::SeqCst),
                ))
                .on_page_load(|view, payload| {
                    if payload.event() == tauri::webview::PageLoadEvent::Finished {
                        let _ = view.eval(background_script(
                            PANEL_DARK.load(std::sync::atomic::Ordering::SeqCst),
                        ));
                    }
                })
                .data_directory(profile)
                .on_navigation(|url| url.scheme() == "https" || url.as_str() == "about:blank"),
            position,
            size,
        )
        .map_err(|e| e.to_string())?;
    if !EMBED_VISIBLE.load(std::sync::atomic::Ordering::SeqCst) {
        view.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn google_ai_action(app: AppHandle, action: String) -> Result<(), String> {
    let Some(view) = app.get_webview(EMBED) else {
        return Ok(());
    };
    match action.as_str() {
        "hide" => {
            EMBED_VISIBLE.store(false, std::sync::atomic::Ordering::SeqCst);
            view.hide()
        }
        "back" => view.eval("history.back()"),
        "reload" => view.eval("location.reload()"),
        "external" => {
            let url = view.url().map_err(|e| e.to_string())?;
            if url.scheme() != "https" {
                return Err("Недопустимый адрес".into());
            }
            return app
                .opener()
                .open_url(url.as_str(), None::<&str>)
                .map_err(|e| e.to_string());
        }
        _ => return Err("Неизвестное действие".into()),
    }
    .map_err(|e| e.to_string())
}

fn search_url(query: &str) -> Result<Url, String> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 1000 || query.chars().any(char::is_control) {
        return Err("Введите запрос длиной до 1000 символов".into());
    }
    let mut url = Url::parse("https://www.google.com/search").map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("udm", "50");
    Ok(url)
}

#[tauri::command]
pub async fn google_ai_open(app: AppHandle, query: String) -> Result<(), String> {
    let url = search_url(&query)?;
    if let Some(win) = app.get_webview_window(LABEL) {
        win.navigate(url).map_err(|e| e.to_string())?;
        win.unminimize().map_err(|e| e.to_string())?;
        win.set_always_on_top(true).map_err(|e| e.to_string())?;
        win.show().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let back = MenuItem::with_id(&app, "google-back", "Назад", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let reload = MenuItem::with_id(&app, "google-reload", "Обновить", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let external = MenuItem::with_id(
        &app,
        "google-external",
        "Открыть в браузере",
        true,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&app, &[&back, &reload, &external]).map_err(|e| e.to_string())?;
    let profile = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("google-browser-profile");
    std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
    let handle = app.clone();
    let win = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::External(url))
        .title("Google AI — Dock Panel (прототип)")
        .inner_size(1100.0, 800.0)
        .min_inner_size(500.0, 400.0)
        .center()
        .always_on_top(true)
        .data_directory(profile)
        .menu(menu)
        .on_navigation(|url| url.scheme() == "https" || url.as_str() == "about:blank")
        .on_menu_event(move |_, event| {
            let Some(win) = handle.get_webview_window(LABEL) else {
                return;
            };
            match event.id().as_ref() {
                "google-back" => {
                    let _ = win.eval("history.back()");
                }
                "google-reload" => {
                    let _ = win.eval("location.reload()");
                }
                "google-external" => {
                    if let Ok(url) = win.url() {
                        if url.scheme() == "https" {
                            let _ = handle.opener().open_url(url.as_str(), None::<&str>);
                        }
                    }
                }
                _ => {}
            }
        })
        .build()
        .map_err(|e| e.to_string())?;
    win.show().map_err(|e| e.to_string())?;
    win.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_is_encoded_without_changing_origin_or_mode() {
        let url = search_url("Stardew Valley & udm=1").unwrap();
        assert_eq!(url.host_str(), Some("www.google.com"));
        let pairs: Vec<_> = url.query_pairs().collect();
        assert_eq!(pairs[0].1, "Stardew Valley & udm=1");
        assert_eq!(pairs[1].1, "50");
        assert!(search_url(" ").is_err());
        assert!(search_url("a\nb").is_err());
    }
}
