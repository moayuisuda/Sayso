//! [`Core`]：平台宿主持有的唯一 core 对象。它拥有设置、密钥、扩展、数据存储与输入会话，
//! 通过 [`Platform`] 使用平台能力，通过 [`CoreObserver`] 推送状态。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crate::asr::source::{self, AsrField, AsrFieldKind, AsrSource};
use crate::asr::AudioFeed;
use crate::error::CoreError;
use crate::extensions::{permission_summary, ExtensionInfo, Extensions};
use crate::l10n::{set_native_language, tr};
use crate::llm::Endpoint;
use crate::package::{self, PackageContents};
use crate::platform::{CoreObserver, Platform};
use crate::plugin::CatalogProblem;
use crate::protocol::{Block, BlockAction, Manifest};
use crate::session::{ActionContext, Session, SessionState};
use crate::settings::{self, PlatformDefaults, RouteKey, SecretSlot, Secrets, Settings};
use crate::store::{StoreWrite, Stores};

/// 数据目录布局，与此前版本一致：已安装扩展与扩展数据都在其中。
struct Paths {
    settings: PathBuf,
    extensions: PathBuf,
}

/// core 的共享状态。会话、扩展分发与操作执行在各自模块中为它实现方法。
pub(crate) struct Inner {
    pub platform: Arc<dyn Platform>,
    pub observer: Arc<dyn CoreObserver>,
    paths: Paths,
    settings: RwLock<Settings>,
    secrets: RwLock<Secrets>,
    pub stores: Stores,
    pub extensions: RwLock<Extensions>,
    pub session: Mutex<Session>,
    /// 当前录音的音频入口，与会话分开加锁：平台在 `stop_audio` 中同步送出尾部音频时不会与会话锁冲突。
    pub audio: Mutex<Option<AudioFeed>>,
    /// 已解压、等待用户确认的扩展包。
    staged_packages: Mutex<HashMap<String, PackageContents>>,
    pub http: reqwest::Client,
    pub runtime: tokio::runtime::Handle,
}

impl Inner {
    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn secrets(&self) -> std::sync::RwLockReadGuard<'_, Secrets> {
        self.secrets.read().unwrap()
    }

    pub fn llm_endpoint(&self) -> Option<Endpoint> {
        settings::llm_endpoint(&self.settings.read().unwrap(), &self.secrets())
    }

    /// 修改并保存设置，通知 UI。
    fn modify_settings(&self, change: impl FnOnce(&mut Settings)) -> Result<(), CoreError> {
        let mut settings = self.settings.write().unwrap();
        let mut updated = settings.clone();
        change(&mut updated);
        settings::save(&self.paths.settings, &updated)?;
        set_native_language(updated.native_language);
        *settings = updated;
        drop(settings);
        self.observer.settings_changed();
        Ok(())
    }

    fn reload_extensions(&self) {
        self.extensions.write().unwrap().reload_installed(&self.paths.extensions);
        self.observer.extensions_changed();
    }
}

/// 扩展页签的内容：按关键词过滤后的条目与过滤前的总数。
#[derive(Clone, Debug, uniffi::Record)]
pub struct TabItems {
    pub items: Vec<TabItem>,
    pub total: u64,
}

/// 扩展页签中的一个条目。
#[derive(Clone, Debug, uniffi::Record)]
pub struct TabItem {
    /// 数据记录的 key，也是条目块的 id。
    pub key: String,
    pub block: Block,
}

/// 已解压、等待用户确认安装的扩展包。
#[derive(Clone, Debug, uniffi::Record)]
pub struct PackageInfo {
    pub token: String,
    pub manifest: Manifest,
    pub byte_size: u64,
    /// 将替换已安装的同一扩展。
    pub replacing: bool,
    pub permission_summary: String,
}

#[derive(uniffi::Object)]
pub struct Core {
    inner: Arc<Inner>,
    /// 只由 Core 持有；Inner 中的任务通过 Handle 提交。
    _runtime: tokio::runtime::Runtime,
}

#[uniffi::export]
impl Core {
    /// `data_dir` 为应用数据目录（macOS：~/Library/Application Support/Sayso）。
    #[uniffi::constructor]
    pub fn new(
        data_dir: String,
        defaults: PlatformDefaults,
        platform: Arc<dyn Platform>,
        observer: Arc<dyn CoreObserver>,
    ) -> Result<Arc<Self>, CoreError> {
        let root = PathBuf::from(data_dir);
        let paths = Paths { settings: root.join("settings.json"), extensions: root.join("Extensions") };
        std::fs::create_dir_all(&paths.extensions)?;

        let settings = settings::load(&paths.settings, defaults);
        set_native_language(settings.native_language);
        let secrets = SecretSlot::all()
            .into_iter()
            .filter_map(|slot| {
                platform.secret_read(slot.account()).filter(|value| !value.is_empty()).map(|v| (slot, v))
            })
            .collect();
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().thread_name("sayso-core").build()?;
        let http = reqwest::Client::builder().build().map_err(|error| CoreError::Io(error.to_string()))?;

        let inner = Arc::new(Inner {
            platform,
            observer,
            extensions: RwLock::new(Extensions::load(&paths.extensions)),
            stores: Stores::new(root.join("ExtensionData")),
            paths,
            settings: RwLock::new(settings),
            secrets: RwLock::new(Secrets(secrets)),
            session: Mutex::new(Session::default()),
            audio: Mutex::default(),
            staged_packages: Mutex::default(),
            http,
            runtime: runtime.handle().clone(),
        });
        Ok(Arc::new(Core { inner, _runtime: runtime }))
    }

    // MARK: 输入会话

    /// 触发一个路由：按路由的输入来源开始录音或读取选区。
    pub fn trigger(&self, route: RouteKey) {
        self.inner.trigger(route);
    }

    /// 按住模式松开入口键：结束录音。
    pub fn stop(&self) {
        self.inner.stop();
    }

    /// 按一下模式：未开始则触发这些路由；录音中则先加上新的路由键，再结束录音。
    pub fn toggle(&self, routes: Vec<RouteKey>) {
        self.inner.toggle(routes);
    }

    pub fn cancel(&self) {
        self.inner.cancel();
    }

    /// 录音数据：16kHz 单声道 16 位小端 PCM。
    pub fn push_audio(&self, pcm: Vec<u8>) {
        if let Some(feed) = self.inner.audio.lock().unwrap().as_ref() {
            feed.send(pcm);
        }
    }

    /// 鼠标悬停在候选框上时不自动隐藏；离开后重新计时。
    pub fn set_hovering(&self, hovering: bool) {
        self.inner.set_hovering(hovering);
    }

    /// 关闭正在展示的候选框。
    pub fn dismiss(&self) {
        self.inner.dismiss();
    }

    pub fn session_state(&self) -> SessionState {
        self.inner.session.lock().unwrap().state()
    }

    /// 正在录音或处理输入。
    pub fn is_session_active(&self) -> bool {
        self.inner.session.lock().unwrap().is_active()
    }

    // MARK: 块操作

    /// 执行候选框中块上的操作。`source_id` 为块所在分组的来源。
    pub fn perform_candidate_action(
        &self,
        surface_id: String,
        source_id: String,
        block_id: String,
        action: BlockAction,
    ) {
        self.inner.perform_action(&source_id, block_id, action, ActionContext::Candidate { surface_id });
    }

    /// 执行扩展页签条目上的操作。删除操作需要 UI 二次确认后调用 [`Core::delete_record`]。
    pub fn perform_tab_action(&self, extension_id: String, record_key: String, action: BlockAction) {
        let Some(collection) = self.tab_collection(&extension_id) else { return };
        let Ok(records) = self.inner.stores.records(&extension_id, &collection) else { return };
        let Some(record) = records.into_iter().find(|record| record.key == record_key) else { return };
        self.inner.perform_action(&extension_id, record_key, action, ActionContext::Tab { record });
    }

    // MARK: 设置

    pub fn settings(&self) -> Settings {
        self.inner.settings()
    }

    pub fn update_settings(&self, settings: Settings) -> Result<(), CoreError> {
        self.inner.modify_settings(|current| *current = settings.normalized(current))
    }

    pub fn secret(&self, slot: SecretSlot) -> String {
        self.inner.secrets().get(slot)
    }

    /// 保存密钥到平台安全存储；空字符串删除。
    pub fn set_secret(&self, slot: SecretSlot, value: String) {
        let value = value.trim().to_string();
        let account = slot.account();
        let mut secrets = self.inner.secrets.write().unwrap();
        if value.is_empty() {
            secrets.0.remove(&slot);
            self.inner.platform.secret_write(account, None);
        } else {
            secrets.0.insert(slot, value.clone());
            self.inner.platform.secret_write(account, Some(value));
        }
        drop(secrets);
        self.inner.observer.settings_changed();
    }

    // MARK: 识别来源

    /// 可选的识别来源：内置服务与已启用的扩展识别器。
    pub fn asr_sources(&self) -> Vec<AsrSource> {
        source::sources(&self.inner.settings(), &self.inner.extensions.read().unwrap())
    }

    /// 识别来源某个配置项的当前取值；来源或配置项不存在时为空。
    pub fn asr_value(&self, source_id: String, field_key: String) -> String {
        let settings = self.inner.settings();
        let Some(field) = self.asr_field(&settings, &source_id, &field_key) else { return String::new() };
        source::field_value(&source_id, &field, &settings, &self.inner.secrets())
    }

    /// 保存识别来源的配置项：密钥类存平台安全存储，其余存设置。
    pub fn set_asr_value(&self, source_id: String, field_key: String, value: String) -> Result<(), CoreError> {
        let settings = self.inner.settings();
        let Some(field) = self.asr_field(&settings, &source_id, &field_key) else { return Ok(()) };
        if field.kind == AsrFieldKind::Secret {
            self.set_secret(SecretSlot::Asr { source: source_id, field: field_key }, value);
            return Ok(());
        }
        self.inner.modify_settings(|settings| {
            settings.asr_options.entry(source_id).or_default().insert(field_key, value);
        })
    }

    /// 需要监听的路由键：绑定了已启用命令的路由。
    pub fn active_route_keys(&self) -> Vec<u16> {
        let settings = self.inner.settings();
        let extensions = self.inner.extensions.read().unwrap();
        settings
            .routes
            .iter()
            .filter_map(|route| match route.key {
                RouteKey::Key { code } if !extensions.commands_routed_to(&settings, &[route.key]).is_empty() => {
                    Some(code)
                }
                _ => None,
            })
            .collect()
    }

    /// 翻译模型的必填项是否齐全。
    pub fn llm_configured(&self) -> bool {
        self.inner.llm_endpoint().is_some()
    }

    /// 用当前翻译模型做一次简短翻译，返回模型回复，用于检查配置。
    pub async fn test_llm(&self) -> Result<String, CoreError> {
        let endpoint = self.inner.llm_endpoint().ok_or(CoreError::ModelNotConfigured)?;
        let native = self.inner.settings().native_language.english_name();
        let http = self.inner.http.clone();
        let system = format!("Translate the text into {native}. Reply with the translation only.");
        self.inner
            .runtime
            .spawn(async move { endpoint.complete(&http, &system, "Hello, world").await })
            .await
            .map_err(|error| CoreError::Llm(error.to_string()))?
    }

    // MARK: 扩展

    pub fn extensions(&self) -> Vec<ExtensionInfo> {
        self.inner.extensions.read().unwrap().infos(&self.inner.settings())
    }

    /// 已安装扩展所在的文件夹。
    pub fn extensions_directory(&self) -> String {
        self.inner.paths.extensions.to_string_lossy().into_owned()
    }

    pub fn extension_problems(&self) -> Vec<CatalogProblem> {
        self.inner.extensions.read().unwrap().problems()
    }

    pub fn set_enabled(&self, extension_id: String, enabled: bool) -> Result<(), CoreError> {
        self.inner.modify_settings(|settings| {
            settings.enabled_extensions.retain(|id| id != &extension_id);
            if enabled {
                settings.enabled_extensions.push(extension_id);
            }
        })?;
        self.inner.observer.extensions_changed();
        Ok(())
    }

    /// 重新扫描扩展目录，例如用户手动放入了扩展文件夹后。
    pub fn reload_extensions(&self) {
        self.inner.reload_extensions();
    }

    /// 扩展页签的条目：按页签声明排序并按关键词过滤。无法按模板渲染的记录被跳过并记录日志。
    pub fn tab_items(&self, extension_id: String, query: String) -> Result<TabItems, CoreError> {
        let extensions = self.inner.extensions.read().unwrap();
        let Some(tab) = extensions.get(&extension_id).and_then(|plugin| plugin.manifest.tab.clone()) else {
            return Ok(TabItems { items: vec![], total: 0 });
        };
        drop(extensions);
        let records = self.inner.stores.records(&extension_id, &tab.collection)?;
        let total = records.len() as u64;
        let items = tab
            .arrange(records, &query)
            .into_iter()
            .filter_map(|record| match tab.render(&record) {
                Ok(block) => Some(TabItem { key: record.key, block }),
                Err(error) => {
                    log::error!("{extension_id} tab item {} not rendered: {error}", record.key);
                    None
                }
            })
            .collect();
        Ok(TabItems { items, total })
    }

    pub fn delete_record(&self, extension_id: String, key: String) -> Result<(), CoreError> {
        let Some(collection) = self.tab_collection(&extension_id) else { return Ok(()) };
        self.inner.stores.apply(&extension_id, StoreWrite::Delete { collection, key })?;
        self.inner.observer.store_changed(extension_id);
        Ok(())
    }

    /// 解压并校验扩展包，返回其内容供用户确认。
    pub async fn open_package(&self, path: String) -> Result<PackageInfo, CoreError> {
        let staging = self.inner.paths.extensions.join(format!(".staging-{}", uuid::Uuid::new_v4()));
        let contents = self
            .inner
            .runtime
            .spawn_blocking(move || package::open(&PathBuf::from(path), staging))
            .await
            .map_err(|error| CoreError::Io(error.to_string()))??;
        let replacing = self.inner.extensions.read().unwrap().installed(&contents.manifest.id).is_some();
        let info = PackageInfo {
            token: uuid::Uuid::new_v4().to_string(),
            manifest: contents.manifest.clone(),
            byte_size: contents.byte_size,
            replacing,
            permission_summary: permission_summary(&contents.manifest),
        };
        self.inner.staged_packages.lock().unwrap().insert(info.token.clone(), contents);
        Ok(info)
    }

    /// 安装已确认的扩展包并启用（用户已在确认框中同意其能力）；已安装同一扩展时整体替换，数据保留。
    pub fn install_package(&self, token: String) -> Result<(), CoreError> {
        let contents = self
            .inner
            .staged_packages
            .lock()
            .unwrap()
            .remove(&token)
            .ok_or_else(|| CoreError::InvalidPackage("package is no longer staged".into()))?;
        let id = contents.manifest.id.clone();
        let existing =
            self.inner.extensions.read().unwrap().installed(&id).and_then(|plugin| plugin.directory().cloned());
        package::install(contents, &existing.unwrap_or_else(|| self.inner.paths.extensions.join(&id)))?;
        self.inner.reload_extensions();
        self.set_enabled(id, true)
    }

    pub fn discard_package(&self, token: String) {
        if let Some(contents) = self.inner.staged_packages.lock().unwrap().remove(&token) {
            package::discard(contents);
        }
    }

    /// 把扩展目录原样导出为扩展包，可直接发给他人安装。
    pub async fn export_package(&self, extension_id: String, destination: String) -> Result<(), CoreError> {
        let directory = self.installed_directory(&extension_id)?;
        self.inner
            .runtime
            .spawn_blocking(move || package::export(&directory, &PathBuf::from(destination)))
            .await
            .map_err(|error| CoreError::Io(error.to_string()))?
    }

    /// 删除已安装扩展的文件；它保存的数据保留在扩展数据目录中。
    pub fn uninstall(&self, extension_id: String) -> Result<(), CoreError> {
        std::fs::remove_dir_all(self.installed_directory(&extension_id)?)?;
        self.inner.reload_extensions();
        self.set_enabled(extension_id, false)
    }
}

impl Core {
    fn asr_field(&self, settings: &Settings, source_id: &str, field_key: &str) -> Option<AsrField> {
        let source = source::find(source_id, settings, &self.inner.extensions.read().unwrap())?;
        source.fields.into_iter().find(|field| field.key == field_key)
    }

    fn tab_collection(&self, extension_id: &str) -> Option<String> {
        let extensions = self.inner.extensions.read().unwrap();
        extensions.get(extension_id).and_then(|plugin| plugin.manifest.tab.as_ref()).map(|tab| tab.collection.clone())
    }

    fn installed_directory(&self, extension_id: &str) -> Result<PathBuf, CoreError> {
        self.inner
            .extensions
            .read()
            .unwrap()
            .installed(extension_id)
            .and_then(|plugin| plugin.directory().cloned())
            .ok_or_else(|| CoreError::Io(tr("扩展未安装", "Extension is not installed")))
    }
}

/// 扩展包的文件扩展名。
#[uniffi::export]
pub fn package_file_extension() -> String {
    package::FILE_EXTENSION.into()
}
