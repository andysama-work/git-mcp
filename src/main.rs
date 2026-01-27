use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    model::{ProtocolVersion, ServerCapabilities, ServerInfo},
    schemars::{self, JsonSchema},
    tool, tool_handler, tool_router,
    transport::stdio,
    handler::server::tool::Parameters,
};
use serde::Deserialize;
use std::future::Future;
use std::process::Command;
use git2::{Repository, StatusOptions};

/// 提交类型定义
struct CommitType {
    emoji: &'static str,
    name: &'static str,
    desc: &'static str,
}

const COMMIT_TYPES: &[CommitType] = &[
    CommitType { emoji: "✨", name: "feat", desc: "新增功能" },
    CommitType { emoji: "🐛", name: "fix", desc: "修复 Bug" },
    CommitType { emoji: "📝", name: "docs", desc: "文档变更" },
    CommitType { emoji: "💄", name: "style", desc: "代码格式" },
    CommitType { emoji: "♻️", name: "refactor", desc: "重构代码" },
    CommitType { emoji: "⚡️", name: "perf", desc: "性能优化" },
    CommitType { emoji: "✅", name: "test", desc: "增加测试" },
    CommitType { emoji: "🔧", name: "chore", desc: "构建/工具变动" },
    CommitType { emoji: "📦", name: "build", desc: "构建系统变动" },
    CommitType { emoji: "👷", name: "ci", desc: "CI 配置变动" },
    CommitType { emoji: "⏪", name: "revert", desc: "回退代码" },
    CommitType { emoji: "🎉", name: "init", desc: "项目初始化" },
    CommitType { emoji: "🎨", name: "ui", desc: "更新 UI 样式" },
    CommitType { emoji: "⚙️", name: "config", desc: "配置文件修改" },
    CommitType { emoji: "🔀", name: "merge", desc: "合并分支" },
];

// ============================================
// 工具参数定义
// ============================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PathParam {
    #[schemars(description = "Git 仓库路径，默认为当前目录")]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CommitMessageParam {
    #[schemars(description = "提交类型: feat/fix/docs/style/refactor/perf/test/chore/build/ci/revert/init/ui/config/merge")]
    pub commit_type: String,
    #[schemars(description = "简短描述（不超过50字符）")]
    pub short_desc: String,
    #[schemars(description = "详细描述列表，每项一个变更点")]
    pub details: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GitCommitParam {
    #[schemars(description = "提交信息")]
    pub message: String,
    #[schemars(description = "Git 仓库路径，默认为当前目录")]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CommitGroup {
    #[schemars(description = "要提交的文件路径列表")]
    pub files: Vec<String>,
    #[schemars(description = "提交类型: feat/fix/docs/style/refactor/perf/test/chore/build/ci/revert/init/ui/config/merge")]
    pub commit_type: String,
    #[schemars(description = "简短描述（不超过50字符）")]
    pub short_desc: String,
    #[schemars(description = "详细描述列表，每项一个变更点")]
    pub details: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SmartCommitParam {
    #[schemars(description = "提交组列表，每组包含文件列表和提交信息，按优先级排序（fix优先，然后feat，最后其他）")]
    pub commits: Vec<CommitGroup>,
    #[schemars(description = "Git 仓库路径，默认为当前目录")]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GitLogParam {
    #[schemars(description = "显示的提交数量，默认10条")]
    pub count: Option<u32>,
    #[schemars(description = "Git 仓库路径，默认为当前目录")]
    pub path: Option<String>,
}

// ============================================
// MCP Server
// ============================================

#[derive(Clone)]
pub struct GitMcpServer {
    tool_router: ToolRouter<Self>,
}

impl GitMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for GitMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl GitMcpServer {
    /// 获取 Git 仓库状态
    #[tool(description = "获取 Git 仓库状态，显示所有变更文件（新增、修改、删除）")]
    async fn git_status(&self, Parameters(param): Parameters<PathParam>) -> String {
        let repo_path = param.path.unwrap_or_else(|| ".".to_string());
        
        let repo = match Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => return format!("❌ 无法打开 Git 仓库: {}", e),
        };

        let mut opts = StatusOptions::new();
        opts.include_untracked(true);

        let statuses = match repo.statuses(Some(&mut opts)) {
            Ok(s) => s,
            Err(e) => return format!("❌ 获取状态失败: {}", e),
        };

        if statuses.is_empty() {
            return "✅ 工作区干净，没有变更".to_string();
        }

        let mut result = String::from("📊 变更导图：\n\n");
        
        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("unknown");
            let status = entry.status();

            let (icon, status_str) = if status.is_index_new() || status.is_wt_new() {
                ("➕", "新增")
            } else if status.is_index_modified() || status.is_wt_modified() {
                ("📝", "修改")
            } else if status.is_index_deleted() || status.is_wt_deleted() {
                ("➖", "删除")
            } else {
                continue;
            };

            result.push_str(&format!("{} {} {}\n", icon, status_str, path));
        }

        result
    }

    /// 生成符合规范的 Git 提交信息
    #[tool(description = "根据提交类型和描述生成符合规范的 Git 提交信息")]
    async fn generate_commit_message(&self, Parameters(param): Parameters<CommitMessageParam>) -> String {
        let type_info = COMMIT_TYPES
            .iter()
            .find(|t| t.name == param.commit_type)
            .unwrap_or(&COMMIT_TYPES[0]);

        let details_str = param.details
            .iter()
            .map(|d| format!("- {}", d))
            .collect::<Vec<_>>()
            .join("\n");

        let commit_msg = format!(
            "{} {}: {}\n\n详细描述：\n{}",
            type_info.emoji, type_info.name, param.short_desc, details_str
        );

        format!("📝 生成的提交信息：\n\n```\n{}\n```", commit_msg)
    }

    /// 执行 Git 提交
    #[tool(description = "执行 git add 和 git commit，使用指定的提交信息")]
    async fn git_commit(&self, Parameters(param): Parameters<GitCommitParam>) -> String {
        let repo_path = param.path.unwrap_or_else(|| ".".to_string());

        // git add .
        let add_output = Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output();

        match add_output {
            Ok(output) if !output.status.success() => {
                return format!("❌ git add 失败: {}", String::from_utf8_lossy(&output.stderr));
            }
            Err(e) => return format!("❌ 执行 git add 失败: {}", e),
            _ => {}
        }

        // git commit
        let commit_output = Command::new("git")
            .args(["commit", "-m", &param.message])
            .current_dir(&repo_path)
            .output();

        match commit_output {
            Ok(output) if output.status.success() => {
                format!("✅ 提交成功！\n\n💡 如需推送，请执行: git push")
            }
            Ok(output) => {
                format!("❌ git commit 失败: {}", String::from_utf8_lossy(&output.stderr))
            }
            Err(e) => format!("❌ 执行 git commit 失败: {}", e),
        }
    }

    /// 获取支持的提交类型列表
    #[tool(description = "获取所有支持的提交类型及其说明")]
    async fn list_commit_types(&self) -> String {
        let mut result = String::from("📋 支持的提交类型：\n\n");
        result.push_str("| Type | Emoji | 说明 |\n");
        result.push_str("|------|-------|------|\n");
        
        for t in COMMIT_TYPES {
            result.push_str(&format!("| {} | {} | {} |\n", t.name, t.emoji, t.desc));
        }
        
        result
    }

    /// 查看 Git 提交历史
    #[tool(description = "查看最近的 Git 提交历史")]
    async fn git_log(&self, Parameters(param): Parameters<GitLogParam>) -> String {
        let repo_path = param.path.unwrap_or_else(|| ".".to_string());
        let n = param.count.unwrap_or(10).to_string();

        let output = Command::new("git")
            .args(["log", "--oneline", "-n", &n])
            .current_dir(&repo_path)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                format!("📜 最近 {} 条提交：\n\n{}", n, String::from_utf8_lossy(&o.stdout))
            }
            Ok(o) => format!("❌ 获取日志失败: {}", String::from_utf8_lossy(&o.stderr)),
            Err(e) => format!("❌ 执行失败: {}", e),
        }
    }

    /// 查看当前分支
    #[tool(description = "查看当前所在的 Git 分支")]
    async fn git_branch(&self, Parameters(param): Parameters<PathParam>) -> String {
        let repo_path = param.path.unwrap_or_else(|| ".".to_string());

        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&repo_path)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                format!("🌿 当前分支: {}", String::from_utf8_lossy(&o.stdout).trim())
            }
            Ok(o) => format!("❌ 获取分支失败: {}", String::from_utf8_lossy(&o.stderr)),
            Err(e) => format!("❌ 执行失败: {}", e),
        }
    }

    /// 智能分类提交
    #[tool(description = "智能分类提交：根据变更类型分组，依次执行多次提交。每组指定文件列表和提交信息，实现 fix/feat/style 等分类提交")]
    async fn smart_commit(&self, Parameters(param): Parameters<SmartCommitParam>) -> String {
        let repo_path = param.path.unwrap_or_else(|| ".".to_string());
        let mut results = Vec::new();
        let mut success_count = 0;

        for (idx, group) in param.commits.iter().enumerate() {
            // 获取提交类型信息
            let type_info = COMMIT_TYPES
                .iter()
                .find(|t| t.name == group.commit_type)
                .unwrap_or(&COMMIT_TYPES[0]);

            // 构建提交信息
            let details_str = group.details
                .iter()
                .map(|d| format!("- {}", d))
                .collect::<Vec<_>>()
                .join("\n");

            let commit_msg = if group.details.is_empty() {
                format!("{} {}: {}", type_info.emoji, type_info.name, group.short_desc)
            } else {
                format!(
                    "{} {}: {}\n\n详细描述：\n{}",
                    type_info.emoji, type_info.name, group.short_desc, details_str
                )
            };

            // git add 指定文件
            let mut add_args = vec!["add".to_string()];
            add_args.extend(group.files.clone());

            let add_output = Command::new("git")
                .args(&add_args)
                .current_dir(&repo_path)
                .output();

            match add_output {
                Ok(output) if !output.status.success() => {
                    results.push(format!(
                        "❌ 第{}组 [{}] git add 失败: {}",
                        idx + 1,
                        group.commit_type,
                        String::from_utf8_lossy(&output.stderr)
                    ));
                    continue;
                }
                Err(e) => {
                    results.push(format!(
                        "❌ 第{}组 [{}] 执行 git add 失败: {}",
                        idx + 1,
                        group.commit_type,
                        e
                    ));
                    continue;
                }
                _ => {}
            }

            // git commit
            let commit_output = Command::new("git")
                .args(["commit", "-m", &commit_msg])
                .current_dir(&repo_path)
                .output();

            match commit_output {
                Ok(output) if output.status.success() => {
                    success_count += 1;
                    results.push(format!(
                        "✅ 第{}组 [{}]: {} ({} 个文件)",
                        idx + 1,
                        group.commit_type,
                        group.short_desc,
                        group.files.len()
                    ));
                }
                Ok(output) => {
                    results.push(format!(
                        "❌ 第{}组 [{}] git commit 失败: {}",
                        idx + 1,
                        group.commit_type,
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
                Err(e) => {
                    results.push(format!(
                        "❌ 第{}组 [{}] 执行 git commit 失败: {}",
                        idx + 1,
                        group.commit_type,
                        e
                    ));
                }
            }
        }

        let summary = format!(
            "📊 分类提交完成：{}/{} 组成功\n\n{}",
            success_count,
            param.commits.len(),
            results.join("\n")
        );

        if success_count > 0 {
            format!("{}\n\n💡 如需推送，请执行: git push", summary)
        } else {
            summary
        }
    }
}

#[tool_handler]
impl ServerHandler for GitMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            instructions: Some("Git MCP Server - 提供 Git 操作工具，支持查看状态、生成规范提交信息、执行提交等功能".to_string()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = GitMcpServer::new().serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
