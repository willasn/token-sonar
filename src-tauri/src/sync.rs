//! Git 仓库同步：把本机的账号凭证导出成 JSON，提交并推送到用户自备的远端 Git 仓库。
//! 同步前会先拉取远端备份（`pull_backup`，只读）供调用方合并，再由调用方推送合并结果。
//!
//! 引擎策略（两条路径，对外接口一致）：
//! - **内置引擎（libgit2）为主**：`https://` 地址默认走 libgit2。应用自带，用户无需安装 Git；
//!   认证走「用户名 + 访问令牌」的凭据回调，凭据不落盘到仓库配置，也不会进入 URL 或日志。
//! - **系统 Git 兜底**：`git@` / `ssh://` 地址必须交给系统 git（复用用户的 SSH 凭据配置）；
//!   `https://` 地址在内置引擎失败后也回退系统 git（可复用 credential helper 与代理配置）。
//!   需要系统 git 而本机没有时，给出明确的安装 / 改用 https 指引。
//!
//! 其它实现要点：
//! - 系统 git 路径下，凭据只在单次命令的 URL 里出现，用完立即把 remote URL 里的密码抹掉
//! - 任何返回给前端的错误文案都会先脱敏，避免密码 / 令牌泄漏到界面或日志
//! - 数据统一写入仓库子目录 `tokensonar/`，不覆盖仓库里的其它内容

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::models::{AppSettings, ProviderConfig, RemoteBackup, SyncConfig, SyncResult};
use crate::storage::AppStore;

/// 导出数据在仓库里的存放目录
const DATA_SUBDIR: &str = "tokensonar";

const AUTHOR_NAME: &str = "TokenSonar";
const AUTHOR_EMAIL: &str = "tokensonar@users.noreply.local";

const SHORT: Duration = Duration::from_secs(30);
const LONG: Duration = Duration::from_secs(300);

/// 需要系统 git 但本机缺失时的统一提示。
fn git_missing_message() -> String {
    "未检测到 Git 命令行工具。\n\
     SSH 地址（git@ / ssh://）的同步需要系统 Git：请先安装 Git 并确保它在 PATH 中\
     （https://git-scm.com/downloads）。\n\
     也可以把仓库地址改成 https:// —— https 使用应用内置引擎，无需安装 Git。"
        .to_string()
}

/// 待写入仓库的一个导出文件
pub struct SyncFile {
    pub name: String,
    pub content: String,
}

fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// ---------------------------------------------------------------------------
// 地址 / 凭据处理（两条引擎共用）
// ---------------------------------------------------------------------------

fn percent_encode(raw: &str) -> String {
    let mut out = String::new();
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn is_ssh_url(url: &str) -> bool {
    url.starts_with("git@") || url.starts_with("ssh://")
}

fn clean_remote_url(cfg: &SyncConfig) -> Result<String, String> {
    let url = cfg.repo_url.trim().to_string();
    if url.is_empty() {
        return Err("请先在「Git 同步」里填写仓库地址".to_string());
    }
    // ssh 地址没有 http userinfo，直接返回（不走背后的 %40 处理会破坏 ssh 语法）
    if url.starts_with("https://") || url.starts_with("http://") || is_ssh_url(&url) {
        Ok(url)
    } else {
        Err("仓库地址需以 https:// 、git@ 或 ssh:// 开头".to_string())
    }
}

/// 把用户名密码拼进 URL 的 userinfo 段，只在系统 git 的单次命令里使用。
fn auth_url(cfg: &SyncConfig) -> Result<String, String> {
    let url = clean_remote_url(cfg)?;
    if !url.starts_with("http") {
        return Ok(url); // ssh：交给本机 SSH 凭据
    }
    let user = cfg.username.trim();
    if user.is_empty() {
        return Ok(url); // 公开仓库匿名访问
    }
    let encoded = format!(
        "{user}:{}",
        percent_encode(&cfg.password).replace('@', "%40")
    );
    Ok(url.replacen("://", &format!("://{encoded}@"), 1))
}

fn branch_of(cfg: &SyncConfig) -> String {
    let b = cfg.branch.trim();
    if b.is_empty() {
        "main".to_string()
    } else {
        b.to_string()
    }
}

/// 把错误文案里的密码 / userinfo 抹掉，防止凭据泄漏到界面上。
fn redact(text: &str, cfg: &SyncConfig) -> String {
    let mut out = text.to_string();
    let pw = cfg.password.trim();
    if !pw.is_empty() {
        out = out.replace(pw, "***");
        out = out.replace(&percent_encode(pw), "***");
    }
    if let Ok(url) = auth_url(cfg) {
        let host_part = url.split("://").nth(1).unwrap_or("");
        if let Some((userinfo, rest)) = host_part.split_once('@') {
            let safe = format!("{}@{}", userinfo.split(':').next().unwrap_or("***"), rest);
            out = out.replace(&format!("{userinfo}@{rest}"), &safe);
            out = out.replace(&host_part, &safe);
        }
    }
    out
}

/// 把原始报错翻译成用户能看懂的中文提示，同时抹掉凭据。
///
/// 同时覆盖系统 git 的英文 stderr 与 libgit2 的英文报错（两套引擎文案不同）。
fn humanize(detail: &str, cfg: &SyncConfig) -> String {
    let text = redact(detail, cfg);
    let lower = text.to_lowercase();

    if text.contains("Could not resolve host")
        || text.contains("couldn't find host")
        || lower.contains("failed to resolve address")
        || lower.contains("could not resolve host")
    {
        return format!("无法解析仓库地址，请检查网络或地址是否正确\n{text}");
    }
    if text.contains("Password authentication is not supported") {
        return format!(
            "该站点不接受账号密码做 Git 认证：请在「密码 / 访问令牌」里填写访问令牌\n\
             （GitHub 用 Personal Access Token，需 repo 读写权限）\n{text}"
        );
    }
    if text.contains("Authentication failed")
        || text.contains("could not read Username")
        || text.contains("401")
        || text.contains("403")
        || lower.contains("authentication required")
        || lower.contains("failed to authenticate")
        || lower.contains("authentication replays")
    {
        return format!(
            "认证失败：请确认用户名与密码/令牌是否正确，且令牌拥有读写仓库的权限\n{text}"
        );
    }
    if text.contains("Repository not found")
        || text.contains("does not appear to be a git")
        || lower.contains("repository not found")
        || lower.contains("unexpected http status code: 404")
    {
        return format!("找不到该仓库，请检查地址拼写或账号是否有访问权限\n{text}");
    }
    // libgit2 推送被拒：远端有本地没有的提交
    if lower.contains("not present locally") || lower.contains("non-fast-forward") {
        return format!("远端仓库有本地没有的更新，推送被拒绝。请稍后重试同步\n{text}");
    }
    if text.contains("Permission denied") {
        return format!("没有权限访问该仓库，请检查令牌权限或 SSH Key 配置\n{text}");
    }
    text
}

// ---------------------------------------------------------------------------
// 导出内容
// ---------------------------------------------------------------------------

fn json_file<T: serde::Serialize>(name: &str, value: &T) -> Result<SyncFile, String> {
    let content =
        serde_json::to_string_pretty(value).map_err(|e| format!("序列化 {name} 失败: {e}"))?;
    Ok(SyncFile {
        name: name.to_string(),
        content,
    })
}

/// 清掉账号里的凭证，供「不含账号密码 / API Key」同步模式使用。
///
/// 只抹掉密钥类字段，站点地址、备注、AK ID 等标识信息保留，便于在另一台机器上还原条目。
fn strip_credentials(p: &mut ProviderConfig) {
    p.token = None;
    p.access_key_secret = None;
    p.session_token = None;
    p.password = None;
    p.platform_token = None;
    if let Some(keys) = p.api_keys.as_mut() {
        for k in keys.iter_mut() {
            k.secret = None;
        }
    }
}

/// 读取本机设置与账号凭证，准备写入 Git 仓库。
///
/// `include_credentials` 为 false 时剔除账号凭证（密码 / Token / 云 AK·SK / 会话令牌），
/// 只同步站点与展示类信息。
///
/// 台账（`ledger.json`）与用量快照（`snapshots.json`）属于本机运行数据，
/// 不进入同步范围——快照每次刷新都会重抓，台账是厂商无接口时的手工兜底记录。
pub fn export_files(store: &AppStore, include_credentials: bool) -> Result<Vec<SyncFile>, String> {
    let mut providers = store.load_providers();
    if !include_credentials {
        for p in &mut providers {
            strip_credentials(p);
        }
    }
    let settings = store.load_settings();

    let manifest = serde_json::json!({
        "app": "TokenSonar",
        "version": env!("CARGO_PKG_VERSION"),
        "exportedAt": now_str(),
        "counts": {
            "providers": providers.len(),
        },
    });

    Ok(vec![
        json_file("providers.json", &providers)?,
        json_file("settings.json", &settings)?,
        json_file("manifest.json", &manifest)?,
    ])
}

fn write_files(repo: &Path, files: &[SyncFile]) -> Result<(), String> {
    let dir = repo.join(DATA_SUBDIR);
    fs::create_dir_all(&dir).map_err(|e| format!("创建导出目录失败: {e}"))?;
    for f in files {
        let path = dir.join(&f.name);
        fs::write(&path, f.content.as_bytes()).map_err(|e| format!("写入 {} 失败: {e}", f.name))?;
    }
    Ok(())
}

fn dir_is_empty(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|mut it| it.next().is_none())
        .unwrap_or(false)
}

/// 读取仓库 `tokensonar/providers.json` 里的账号列表，用于合并同步。
fn read_providers_from_repo(repo: &Path) -> Result<Vec<ProviderConfig>, String> {
    let path = repo.join(DATA_SUBDIR).join("providers.json");
    let content = fs::read_to_string(&path).map_err(|_| {
        format!("仓库里没有 {DATA_SUBDIR}/providers.json，可能不是本应用的备份仓库，或还没有备份")
    })?;
    let mut list: Vec<ProviderConfig> =
        serde_json::from_str(&content).map_err(|e| format!("解析 providers.json 失败：{e}"))?;
    for p in &mut list {
        p.normalize();
    }
    Ok(list)
}

/// 读取仓库 `tokensonar/settings.json`。缺失或解析失败都返回 None——
/// 首次同步时远端还没有备份，此时本机设置即基线。
fn read_settings_from_repo(repo: &Path) -> Option<AppSettings> {
    let path = repo.join(DATA_SUBDIR).join("settings.json");
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 仓库身份：去掉 userinfo、结尾的 / 与 .git，用于判断本地副本是否还是同一个仓库
fn repo_identity(url: &str) -> String {
    let lower = url.trim().to_lowercase();
    let plain = match lower.split_once("://") {
        Some((scheme, rest)) => match rest.split_once('@') {
            Some((_userinfo, host)) => format!("{scheme}://{host}"),
            None => lower.clone(),
        },
        None => lower.clone(),
    };
    plain
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string()
}

// ===========================================================================
// 引擎一：内置 libgit2（https 主路径）
// ===========================================================================

/// libgit2 的凭据回调：https 用「用户名 + 访问令牌」，公开仓库则匿名。
///
/// 凭据只在这段闭包里存活，不会写进仓库配置，也不会拼进 URL。
fn git2_callbacks(cfg: &SyncConfig) -> git2::RemoteCallbacks<'static> {
    let user = cfg.username.trim().to_string();
    let pass = cfg.password.clone();
    let mut cb = git2::RemoteCallbacks::new();
    cb.credentials(move |_url, _from_user, allowed| {
        if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) && !user.is_empty() {
            return git2::Cred::userpass_plaintext(&user, &pass);
        }
        if allowed.contains(git2::CredentialType::USERNAME) && !user.is_empty() {
            return git2::Cred::username(&user);
        }
        git2::Cred::default()
    });
    cb
}

/// 探测远端仓库的可访问性并列出分支名，同时验证认证是否通过。
fn git2_list_heads(url: &str, cfg: &SyncConfig) -> Result<Vec<String>, String> {
    let mut remote = git2::Remote::create_detached(url)
        .map_err(|e| humanize(&format!("无法连接仓库：{}", e.message()), cfg))?;
    remote
        .connect_auth(git2::Direction::Fetch, Some(git2_callbacks(cfg)), None)
        .map_err(|e| humanize(&format!("无法连接仓库：{}", e.message()), cfg))?;
    let heads = remote
        .list()
        .map_err(|e| humanize(&format!("读取远端分支失败：{}", e.message()), cfg))?;
    Ok(heads
        .iter()
        .filter_map(|h| h.name().strip_prefix("refs/heads/"))
        .map(|s| s.to_string())
        .collect())
}

fn git2_test_connection(cfg: &SyncConfig) -> Result<String, String> {
    let url = clean_remote_url(cfg)?;
    let branch = branch_of(cfg);
    let branches = git2_list_heads(&url, cfg)?;

    if branches.is_empty() {
        return Ok(format!(
            "连接成功：仓库可访问，目前还没有任何分支，首次同步会创建 {branch}"
        ));
    }
    if branches.iter().any(|b| b == &branch) {
        Ok(format!(
            "连接成功：远端已存在 {branch} 分支（共 {} 个分支）",
            branches.len()
        ))
    } else {
        Ok(format!(
            "连接成功：远端暂无 {} 分支，首次同步会自动创建（现有分支：{}）",
            branch,
            branches.join("、")
        ))
    }
}

/// 打开或初始化本地暂存仓库；换了同步目标则重建副本（先确认新地址可达再删）。
fn git2_open_repo(repo_dir: &Path, cfg: &SyncConfig, url: &str) -> Result<git2::Repository, String> {
    let git_dir = repo_dir.join(".git");

    if git_dir.exists() {
        let repo = git2::Repository::open(repo_dir).map_err(|e| {
            humanize(
                &format!(
                    "本地暂存仓库已损坏：{}\n可删除 {} 后重试",
                    e.message(),
                    repo_dir.display()
                ),
                cfg,
            )
        })?;

        let same = repo
            .find_remote("origin")
            .ok()
            .and_then(|r| r.url().ok().map(|u| repo_identity(u) == repo_identity(url)))
            .unwrap_or(false);
        if same {
            return Ok(repo);
        }

        // 换了仓库：先确认新地址可用（调用方已探测过），避免连不上时把副本也弄丢
        drop(repo);
        fs::remove_dir_all(repo_dir).map_err(|e| format!("清理旧同步副本失败: {e}"))?;
    }

    fs::create_dir_all(repo_dir).map_err(|e| format!("创建本地暂存目录失败: {e}"))?;
    if !dir_is_empty(repo_dir) {
        return Err(format!(
            "本地暂存目录非空且不是 Git 仓库：{}\n请删除该目录后重试",
            repo_dir.display()
        ));
    }

    let repo = git2::Repository::init(repo_dir)
        .map_err(|e| humanize(&format!("初始化本地仓库失败：{}", e.message()), cfg))?;
    repo.remote("origin", url)
        .map_err(|e| humanize(&format!("设置远端地址失败：{}", e.message()), cfg))?;
    Ok(repo)
}

/// 用 libgit2 完成一次「导出 → 提交 → 推送」。
fn git2_push_backup(
    repo_dir: &Path,
    cfg: &SyncConfig,
    files: &[SyncFile],
) -> Result<SyncResult, String> {
    let url = clean_remote_url(cfg)?;
    let branch = branch_of(cfg);
    let branch_ref = format!("refs/heads/{branch}");

    // 先探测：既验证可达 / 认证，也判断目标分支是否已存在
    let branches = git2_list_heads(&url, cfg)?;
    let remote_has_branch = branches.iter().any(|b| b == &branch);

    let repo = git2_open_repo(repo_dir, cfg, &url)?;

    // 把远端作为基线，避免直接推送被拒；本地导出的内容随后覆盖自身子目录
    let mut base_oid: Option<git2::Oid> = None;
    if remote_has_branch {
        let refspec = format!("+{branch_ref}:refs/remotes/origin/{branch}");
        let mut remote = repo
            .find_remote("origin")
            .map_err(|e| humanize(&format!("读取远端配置失败：{}", e.message()), cfg))?;
        let mut fo = git2::FetchOptions::new();
        fo.remote_callbacks(git2_callbacks(cfg));
        remote
            .fetch(&[refspec.as_str()], Some(&mut fo), None)
            .map_err(|e| humanize(&format!("拉取远端更新失败：{}", e.message()), cfg))?;

        let reference = repo
            .find_reference(&format!("refs/remotes/origin/{branch}"))
            .map_err(|e| humanize(&format!("读取远端分支失败：{}", e.message()), cfg))?;
        base_oid = reference.target();
    }

    match base_oid {
        Some(oid) => {
            // 本地分支指向远端基线，并把工作区重置到该提交
            let obj = repo
                .find_object(oid, None)
                .map_err(|e| humanize(&format!("读取远端提交失败：{}", e.message()), cfg))?;
            repo.reference(&branch_ref, oid, true, "tokensonar: 同步基线")
                .map_err(|e| humanize(&format!("更新本地分支失败：{}", e.message()), cfg))?;
            repo.set_head(&branch_ref)
                .map_err(|e| humanize(&format!("切换本地分支失败：{}", e.message()), cfg))?;
            repo.reset(&obj, git2::ResetType::Hard, None)
                .map_err(|e| humanize(&format!("重置本地工作区失败：{}", e.message()), cfg))?;
        }
        None => {
            // 远端还没有这个分支：清掉同名旧引用，稍后建立首个提交
            if let Ok(mut r) = repo.find_reference(&branch_ref) {
                let _ = r.delete();
            }
        }
    }

    write_files(repo_dir, files)?;

    let mut index = repo
        .index()
        .map_err(|e| humanize(&format!("读取索引失败：{}", e.message()), cfg))?;
    index
        .add_all([DATA_SUBDIR], git2::IndexAddOption::DEFAULT, None)
        .map_err(|e| humanize(&format!("暂存文件失败：{}", e.message()), cfg))?;
    index
        .write()
        .map_err(|e| humanize(&format!("写入索引失败：{}", e.message()), cfg))?;
    let tree_oid = index
        .write_tree()
        .map_err(|e| humanize(&format!("生成目录树失败：{}", e.message()), cfg))?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|e| humanize(&format!("读取目录树失败：{}", e.message()), cfg))?;

    // 我们只覆盖自身子目录，其余内容来自远端基线；
    // 因此新目录树与基线一致时即可判定「数据无变化」。
    let parent = match base_oid {
        Some(oid) => Some(
            repo.find_commit(oid)
                .map_err(|e| humanize(&format!("读取基线提交失败：{}", e.message()), cfg))?,
        ),
        None => None,
    };
    let dirty = match &parent {
        Some(c) => c.tree_id() != tree_oid,
        None => true,
    };

    if !dirty {
        return Ok(SyncResult {
            ok: true,
            message: format!("数据无变化，已与 {branch} 保持一致"),
            commit: base_oid.map(|o| o.to_string()[..7].to_string()),
            changed: false,
            file_count: files.len(),
            synced_at: now_str(),
        });
    }

    let sig = git2::Signature::now(AUTHOR_NAME, AUTHOR_EMAIL)
        .map_err(|e| humanize(&format!("构造提交身份失败：{}", e.message()), cfg))?;
    let message = format!("chore(sync): TokenSonar 数据备份 {}", now_str());
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    let commit_oid = repo
        .commit(Some(branch_ref.as_str()), &sig, &sig, &message, &tree, &parents)
        .map_err(|e| humanize(&format!("提交失败：{}", e.message()), cfg))?;

    let mut remote = repo
        .find_remote("origin")
        .map_err(|e| humanize(&format!("读取远端配置失败：{}", e.message()), cfg))?;
    let push_refspec = format!("{branch_ref}:{branch_ref}");
    let mut po = git2::PushOptions::new();
    po.remote_callbacks(git2_callbacks(cfg));
    remote
        .push(&[push_refspec.as_str()], Some(&mut po))
        .map_err(|e| humanize(&format!("推送失败：{}", e.message()), cfg))?;

    let short = commit_oid.to_string()[..7].to_string();
    Ok(SyncResult {
        ok: true,
        message: format!(
            "已推送到 {branch}：{} 个文件，提交 {short}",
            files.len()
        ),
        commit: Some(short),
        changed: true,
        file_count: files.len(),
        synced_at: now_str(),
    })
}

/// 用 libgit2 把远端分支拉到本地工作区，并读出备份里的账号列表。
fn git2_pull_backup(repo_dir: &Path, cfg: &SyncConfig) -> Result<RemoteBackup, String> {
    let url = clean_remote_url(cfg)?;
    let branch = branch_of(cfg);
    let branch_ref = format!("refs/heads/{branch}");

    // 远端必须有目标分支，否则没有可恢复的备份
    let branches = git2_list_heads(&url, cfg)?;
    if !branches.iter().any(|b| b == &branch) {
        return Err(format!("远端还没有 {branch} 分支，暂无可恢复的备份"));
    }

    let repo = git2_open_repo(repo_dir, cfg, &url)?;

    let refspec = format!("+{branch_ref}:refs/remotes/origin/{branch}");
    let mut remote = repo
        .find_remote("origin")
        .map_err(|e| humanize(&format!("读取远端配置失败：{}", e.message()), cfg))?;
    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(git2_callbacks(cfg));
    remote
        .fetch(&[refspec.as_str()], Some(&mut fo), None)
        .map_err(|e| humanize(&format!("拉取远端备份失败：{}", e.message()), cfg))?;

    let reference = repo
        .find_reference(&format!("refs/remotes/origin/{branch}"))
        .map_err(|e| humanize(&format!("读取远端分支失败：{}", e.message()), cfg))?;
    let oid = reference
        .target()
        .ok_or_else(|| format!("远端 {branch} 分支还没有任何提交，暂无可恢复的备份"))?;
    let obj = repo
        .find_object(oid, None)
        .map_err(|e| humanize(&format!("读取远端提交失败：{}", e.message()), cfg))?;

    // 本地分支与工作区对齐到远端备份
    repo.reference(&branch_ref, oid, true, "tokensonar: 恢复基线")
        .map_err(|e| humanize(&format!("更新本地分支失败：{}", e.message()), cfg))?;
    repo.set_head(&branch_ref)
        .map_err(|e| humanize(&format!("切换本地分支失败：{}", e.message()), cfg))?;
    repo.reset(&obj, git2::ResetType::Hard, None)
        .map_err(|e| humanize(&format!("重置本地工作区失败：{}", e.message()), cfg))?;

    let providers = read_providers_from_repo(repo_dir)?;
    Ok(RemoteBackup {
        providers,
        settings: read_settings_from_repo(repo_dir),
        branch,
        commit: Some(oid.to_string()[..7].to_string()),
    })
}

// ===========================================================================
// 引擎二：系统 git 命令行（SSH / 兜底）
// ===========================================================================

fn git(dir: Option<&Path>) -> Command {
    let mut cmd = Command::new("git");
    // 关掉交互式凭据询问：拿到坏凭据时宁可失败也不要卡住界面
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("GIT_AUTHOR_NAME", AUTHOR_NAME)
        .env("GIT_AUTHOR_EMAIL", AUTHOR_EMAIL)
        .env("GIT_COMMITTER_NAME", AUTHOR_NAME)
        .env("GIT_COMMITTER_EMAIL", AUTHOR_EMAIL);
    // 仓库目录可能属于其它用户，放开 ownership 校验以免直接失败
    cmd.arg("-c").arg("safe.directory=*");
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd
}

/// 执行一条 git 命令，超时则杀掉子进程。成功返回输出，失败返回输出作为错误。
fn run(dir: Option<&Path>, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut cmd = git(dir);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("无法执行 git：{e}"))?;

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("git {} 执行超时", args.join(" ")));
                }
                thread::sleep(Duration::from_millis(80));
            }
            Err(e) => return Err(format!("等待 git 失败：{e}")),
        }
    }

    let out = child
        .wait_with_output()
        .map_err(|e| format!("读取 git 输出失败：{e}"))?;
    let text = {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let merged = if stderr.trim().is_empty() {
            stdout.to_string()
        } else if stdout.trim().is_empty() {
            stderr.to_string()
        } else {
            format!("{stderr}\n{stdout}")
        };
        merged.trim().to_string()
    };

    if out.status.success() {
        Ok(text)
    } else {
        Err(if text.is_empty() {
            format!(
                "git {} 失败（退出码 {:?}）",
                args.join(" "),
                out.status.code()
            )
        } else {
            text
        })
    }
}

/// 本机是否存在可用的 git 命令行
fn git_cli_available() -> bool {
    run(None, &["--version"], SHORT).is_ok()
}

/// 需要系统 git 时确保其存在，缺失则给出明确安装 / 改用 https 的提示。
fn ensure_git_cli() -> Result<(), String> {
    if git_cli_available() {
        Ok(())
    } else {
        Err(git_missing_message())
    }
}

fn git_cli_test_connection(cfg: &SyncConfig) -> Result<String, String> {
    let url = auth_url(cfg)?;
    let branch = branch_of(cfg);

    let refs = run(None, &["ls-remote", "--heads", &url], SHORT)
        .map_err(|e| humanize(&format!("无法连接仓库：{e}"), cfg))?;

    let branches: Vec<String> = refs
        .lines()
        .filter_map(|l| l.split("refs/heads/").nth(1))
        .map(|s| s.trim().to_string())
        .collect();

    if branches.is_empty() {
        return Ok(format!(
            "连接成功：仓库可访问，目前还没有任何分支，首次同步会创建 {branch}"
        ));
    }
    if branches.iter().any(|b| b == &branch) {
        Ok(format!(
            "连接成功：远端已存在 {branch} 分支（共 {} 个分支）",
            branches.len()
        ))
    } else {
        Ok(format!(
            "连接成功：远端暂无 {} 分支，首次同步会自动创建（现有分支：{}）",
            branch,
            branches.join("、")
        ))
    }
}

fn remote_has_branch(cfg: &SyncConfig, url: &str, branch: &str) -> Result<bool, String> {
    let refs = run(None, &["ls-remote", "--heads", url, branch], SHORT)
        .map_err(|e| humanize(&format!("读取远端分支失败：{e}"), cfg))?;
    Ok(!refs.trim().is_empty())
}

/// 抹掉 git 配置里可能残留的凭据（克隆时 URL 带着用户名密码会被写进 .git/config）
fn scrub_remote(repo: &Path) {
    let Ok(url) = run(Some(repo), &["remote", "get-url", "origin"], SHORT) else {
        return;
    };
    if let Some(rest) = url.split("://").nth(1) {
        if let Some((_userinfo, host)) = rest.split_once('@') {
            let scheme = url.split("://").next().unwrap_or("https");
            let safe = format!("{scheme}://{host}");
            let _ = run(Some(repo), &["remote", "set-url", "origin", &safe], SHORT);
        }
    }
}

/// 保证 `repo` 是一个可用的 Git 工作副本：已有则复用，没有则克隆。
fn prepare_repo(repo: &Path, cfg: &SyncConfig, url: &str) -> Result<(), String> {
    if repo.join(".git").exists() {
        run(Some(repo), &["rev-parse", "--is-inside-work-tree"], SHORT).map_err(|e| {
            humanize(
                &format!("本地暂存仓库已损坏：{e}\n可删除 {} 后重试", repo.display()),
                cfg,
            )
        })?;

        // 换了同步目标：旧副本的历史不能带到新仓库，删掉重新克隆
        let same = run(Some(repo), &["remote", "get-url", "origin"], SHORT)
            .map(|u| repo_identity(&u) == repo_identity(url))
            .unwrap_or(false);
        if same {
            scrub_remote(repo);
            return Ok(());
        }
        // 确认新地址可用再删，避免连不上新仓库时把副本也弄丢
        match run(None, &["ls-remote", url], SHORT) {
            Ok(_) => fs::remove_dir_all(repo).map_err(|e| format!("清理旧同步副本失败: {e}"))?,
            Err(e) => {
                return Err(humanize(
                    &format!("无法访问新的仓库地址，已保留原有副本：{e}"),
                    cfg,
                ))
            }
        }
    }

    fs::create_dir_all(repo).map_err(|e| format!("创建本地暂存目录失败: {e}"))?;
    if !dir_is_empty(repo) {
        return Err(format!(
            "本地暂存目录非空且不是 Git 仓库：{}\n请删除该目录后重试",
            repo.display()
        ));
    }

    let parent = repo.parent().unwrap_or(repo).to_path_buf();
    let target = repo.display().to_string();
    let branch = branch_of(cfg);

    if remote_has_branch(cfg, url, &branch)? {
        // 远端已有目标分支：直接克隆该分支
        run(
            Some(&parent),
            &["clone", "--no-tags", "--branch", &branch, url, &target],
            LONG,
        )
        .map_err(|e| humanize(&format!("克隆仓库失败：{e}"), cfg))?;
    } else {
        // 空仓库或目标分支还不存在：克隆默认分支后本地切到目标分支
        run(Some(&parent), &["clone", "--no-tags", url, &target], LONG)
            .map_err(|e| humanize(&format!("克隆仓库失败：{e}"), cfg))?;
        checkout_local_branch(repo, &branch)
            .map_err(|e| humanize(&format!("切换到 {branch} 分支失败：{e}"), cfg))?;
    }
    scrub_remote(repo);
    Ok(())
}

fn checkout_local_branch(repo: &Path, branch: &str) -> Result<(), String> {
    let born = run(
        Some(repo),
        &["rev-parse", "--verify", "--quiet", "HEAD"],
        SHORT,
    )
    .is_ok();
    if born {
        run(Some(repo), &["checkout", "-B", branch], SHORT).map(|_| ())
    } else {
        // 空仓库没有 HEAD，直接把 HEAD 指向新分支
        run(
            Some(repo),
            &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
            SHORT,
        )
        .map(|_| ())
    }
}

/// 远端分支存在时先拉取合并，避免直接推送被拒。
fn pull_remote(repo: &Path, cfg: &SyncConfig, url: &str, branch: &str) -> Result<(), String> {
    if !remote_has_branch(cfg, url, branch)? {
        return Ok(()); // 远端还没有这个分支，首次推送即可
    }

    let has_history = run(
        Some(repo),
        &["rev-parse", "--verify", "--quiet", "HEAD"],
        SHORT,
    )
    .is_ok();
    if !has_history {
        return Ok(());
    }

    // 先把远端 ref 拉到 FETCH_HEAD，判断两边是否有共同祖先
    run(Some(repo), &["fetch", url, branch], LONG)
        .map_err(|e| humanize(&format!("拉取远端更新失败：{e}"), cfg))?;

    if run(Some(repo), &["merge-base", "HEAD", "FETCH_HEAD"], SHORT).is_ok() {
        run(
            Some(repo),
            &["pull", "--rebase", "--autostash", url, branch],
            LONG,
        )
        .map_err(|e| humanize(&format!("拉取远端更新失败：{e}"), cfg))?;
    } else {
        // 仓库里已有其它内容（README 等），允许合并不相关历史，
        // 冲突时以本地导出的数据为准
        run(
            Some(repo),
            &[
                "pull",
                "--no-rebase",
                "--allow-unrelated-histories",
                "-X",
                "ours",
                url,
                branch,
            ],
            LONG,
        )
        .map_err(|e| humanize(&format!("合并远端历史失败：{e}"), cfg))?;
    }
    Ok(())
}

/// 用系统 git 完成一次「导出 → 提交 → 推送」。
fn git_cli_push_backup(
    repo_dir: &Path,
    cfg: &SyncConfig,
    files: &[SyncFile],
) -> Result<SyncResult, String> {
    let url = auth_url(cfg)?;
    let branch = branch_of(cfg);

    prepare_repo(repo_dir, cfg, &url)?;
    pull_remote(repo_dir, cfg, &url, &branch)?;
    write_files(repo_dir, files)?;

    run(Some(repo_dir), &["add", "-A", "--", DATA_SUBDIR], SHORT)
        .map_err(|e| humanize(&format!("暂存文件失败：{e}"), cfg))?;
    let dirty = !run(
        Some(repo_dir),
        &["status", "--porcelain", "--", DATA_SUBDIR],
        SHORT,
    )
    .unwrap_or_default()
    .trim()
    .is_empty();

    let mut message = String::new();
    if dirty {
        let commit_msg = format!("chore(sync): TokenSonar 数据备份 {}", now_str());
        run(Some(repo_dir), &["commit", "-m", &commit_msg], SHORT)
            .map_err(|e| humanize(&format!("提交失败：{e}"), cfg))?;
    }

    let push = run(
        Some(repo_dir),
        &["push", &url, &format!("HEAD:{branch}")],
        LONG,
    );
    if let Err(e) = push {
        return Err(humanize(&format!("推送失败：{e}"), cfg));
    }

    let commit = run(Some(repo_dir), &["rev-parse", "--short", "HEAD"], SHORT).ok();
    message.push_str(&if dirty {
        format!(
            "已推送到 {branch}：{} 个文件{}",
            files.len(),
            commit
                .as_ref()
                .map(|c| format!("，提交 {c}"))
                .unwrap_or_default()
        )
    } else {
        format!("数据无变化，已与 {branch} 保持一致")
    });

    Ok(SyncResult {
        ok: true,
        message,
        commit,
        changed: dirty,
        file_count: files.len(),
        synced_at: now_str(),
    })
}

/// 用系统 git 把远端分支拉到本地工作区，并读出备份里的账号列表。
fn git_cli_pull_backup(repo_dir: &Path, cfg: &SyncConfig) -> Result<RemoteBackup, String> {
    let url = auth_url(cfg)?;
    let branch = branch_of(cfg);

    if !remote_has_branch(cfg, &url, &branch)? {
        return Err(format!("远端还没有 {branch} 分支，暂无可恢复的备份"));
    }

    prepare_repo(repo_dir, cfg, &url)?;
    run(Some(repo_dir), &["fetch", &url, &branch], LONG)
        .map_err(|e| humanize(&format!("拉取远端备份失败：{e}"), cfg))?;
    run(Some(repo_dir), &["reset", "--hard", "FETCH_HEAD"], LONG)
        .map_err(|e| humanize(&format!("重置本地工作区失败：{e}"), cfg))?;

    let commit = run(Some(repo_dir), &["rev-parse", "--short", "HEAD"], SHORT).ok();
    let providers = read_providers_from_repo(repo_dir)?;
    Ok(RemoteBackup {
        providers,
        settings: read_settings_from_repo(repo_dir),
        branch,
        commit,
    })
}

// ===========================================================================
// 引擎调度
// ===========================================================================

/// 连通性测试：https 走内置引擎，ssh 交系统 git；内置引擎失败时回退系统 git。
///
/// 只做一次远端探测，不写入任何文件。
pub fn test_connection(cfg: &SyncConfig) -> Result<String, String> {
    let url = clean_remote_url(cfg)?;

    if is_ssh_url(&url) {
        ensure_git_cli()?;
        return git_cli_test_connection(cfg);
    }

    match git2_test_connection(cfg) {
        Ok(msg) => Ok(msg),
        Err(err) => {
            if git_cli_available() {
                git_cli_test_connection(cfg)
                    .map(|msg| format!("{msg}\n（内置引擎不可用，已改用系统 Git）"))
                    .map_err(|e| format!("{err}\n\n已尝试系统 Git：{e}"))
            } else {
                Err(err)
            }
        }
    }
}

/// 导出本机数据 → 提交 → 推送到远端仓库。
///
/// - `https://`：内置引擎为主，失败且有系统 git 时回退；
/// - `git@` / `ssh://`：必须使用系统 git，缺失时给出明确提示。
pub fn push_backup(
    repo_dir: &Path,
    cfg: &SyncConfig,
    files: &[SyncFile],
) -> Result<SyncResult, String> {
    let url = clean_remote_url(cfg)?;

    if is_ssh_url(&url) {
        ensure_git_cli()?;
        let mut r = git_cli_push_backup(repo_dir, cfg, files)?;
        r.message.push_str("（系统 Git）");
        return Ok(r);
    }

    match git2_push_backup(repo_dir, cfg, files) {
        Ok(mut r) => {
            r.message.push_str("（内置引擎）");
            Ok(r)
        }
        Err(err) => {
            if git_cli_available() {
                match git_cli_push_backup(repo_dir, cfg, files) {
                    Ok(mut r) => {
                        r.message.push_str("（已回退系统 Git）");
                        Ok(r)
                    }
                    Err(e2) => Err(format!("{err}\n\n已回退系统 Git 仍失败：{e2}")),
                }
            } else {
                Err(err)
            }
        }
    }
}

/// 从远端仓库拉取备份里的账号列表（只读，不写本机存储）。
///
/// - `https://`：内置引擎为主，失败且有系统 git 时回退；
/// - `git@` / `ssh://`：必须使用系统 git，缺失时给出明确提示。
pub fn pull_backup(repo_dir: &Path, cfg: &SyncConfig) -> Result<RemoteBackup, String> {
    let url = clean_remote_url(cfg)?;

    if is_ssh_url(&url) {
        ensure_git_cli()?;
        return git_cli_pull_backup(repo_dir, cfg);
    }

    match git2_pull_backup(repo_dir, cfg) {
        Ok(backup) => Ok(backup),
        Err(err) => {
            if git_cli_available() {
                git_cli_pull_backup(repo_dir, cfg)
                    .map_err(|e| format!("{err}\n\n已尝试系统 Git：{e}"))
            } else {
                Err(err)
            }
        }
    }
}
