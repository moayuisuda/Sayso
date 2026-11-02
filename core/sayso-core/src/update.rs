//! 版本更新检查：读取 GitHub Releases 上的最新正式版，比当前版本新时由宿主提醒用户。

use std::time::Duration;

use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;

use crate::error::CoreError;
use crate::l10n::tr;

/// 最新正式版接口：不包含草稿与预发布版本。
const LATEST_RELEASE_API: &str = "https://api.github.com/repos/moayuisuda/Sayso/releases/latest";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// 应用常驻菜单栏或托盘，启动后每天检查一次。
pub(crate) const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// 一个已发布的版本。
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AppRelease {
    /// 不带 `v` 前缀的版本号，如 `1.2.0`。
    pub version: String,
    /// 发布页地址，列出各平台安装包。
    pub url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

pub(crate) async fn latest_release(http: &reqwest::Client) -> Result<AppRelease, CoreError> {
    let failed = |error: reqwest::Error| CoreError::Update(format!("{}: {error}", tr("检查更新失败", "Update check failed")));
    let release: GithubRelease = http
        .get(LATEST_RELEASE_API)
        // GitHub API 拒绝没有 User-Agent 的请求。
        .header(USER_AGENT, "Sayso")
        .header(ACCEPT, "application/vnd.github+json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(failed)?
        .json()
        .await
        .map_err(failed)?;
    Ok(AppRelease { version: release.tag_name.trim_start_matches('v').to_string(), url: release.html_url })
}

/// `candidate` 是否比 `current` 新。版本号为最多三段的点分数字；任一方无法解析时不视为更新。
pub(crate) fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

/// 解析为 [主, 次, 修订]，缺省段为 0。
fn parse(version: &str) -> Option<[u64; 3]> {
    let mut parts = [0; 3];
    for (index, part) in version.trim_start_matches('v').split('.').enumerate() {
        *parts.get_mut(index)? = part.parse().ok()?;
    }
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_numerically_by_segment() {
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("1.2", "1.2.0"));
        assert!(!is_newer("0.1.5", "0.1.5"));
        // 无法解析的版本号（如本地未写入版本）不提示更新。
        assert!(!is_newer("1.0.0-beta", "0.1.0"));
        assert!(!is_newer("1.0.0", ""));
    }
}
