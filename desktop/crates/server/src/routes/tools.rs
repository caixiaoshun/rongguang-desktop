//! AI 助手的工具集（与 Java 版 44 个 ToolExecutor 对齐）。
//!
//! 每个工具都是"薄封装"：解析参数 → 调用数据层 → 返回结构化 JSON 文本。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::{Column, Row};

use fusion_ai::agent::ToolExecutor;
use fusion_ai::types::ToolDef;
use fusion_core::error::{AppError, AppResult};

use crate::state::AppState;
use crate::support::now;

pub struct AssistantTools {
    pub state: Arc<AppState>,
    pub user_id: i64,
}

fn def(name: &str, description: &str, properties: Value, required: Vec<&str>) -> ToolDef {
    ToolDef::new(
        name,
        description,
        json!({ "type": "object", "properties": properties, "required": required }),
    )
}

impl AssistantTools {
    pub fn definitions_inner() -> Vec<ToolDef> {
        vec![
            // ---------- 项目 ----------
            def(
                "ListProjects",
                "列出当前工作区的所有项目（ID、名称、状态、更新时间）。",
                json!({ "keyword": { "type": "string", "description": "可选，按名称模糊筛选" } }),
                vec![],
            ),
            def(
                "ProjectQuery",
                "查询单个项目的完整信息，包含画风与扩展属性。",
                json!({ "projectId": { "type": "integer", "description": "项目 ID" } }),
                vec!["projectId"],
            ),
            def(
                "SaveProject",
                "创建或更新项目。不传 projectId 表示新建。",
                json!({
                    "projectId": { "type": "integer" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "status": { "type": "integer", "description": "0-筹备 1-进行中 2-完成 3-归档" },
                    "artStyle": { "type": "string" },
                    "artStyleDescription": { "type": "string" },
                    "artStyleImagePrompt": { "type": "string" }
                }),
                vec!["name"],
            ),
            def(
                "DeleteProject",
                "删除项目（逻辑删除，同时隐藏其剧本、分镜与资产）。",
                json!({ "projectId": { "type": "integer" } }),
                vec!["projectId"],
            ),
            // ---------- 剧本 ----------
            def(
                "GetProjectScript",
                "获取项目关联的剧本主记录（含梗概、角色、集数）。",
                json!({ "projectId": { "type": "integer" } }),
                vec!["projectId"],
            ),
            def(
                "ScriptQuery",
                "查询剧本详情，可按剧本 ID 或项目 ID 定位。",
                json!({ "scriptId": { "type": "integer" }, "projectId": { "type": "integer" } }),
                vec![],
            ),
            def(
                "ScriptStructureQuery",
                "获取剧本的结构树：分集列表 + 每集场次统计。",
                json!({ "scriptId": { "type": "integer" } }),
                vec!["scriptId"],
            ),
            def(
                "ScriptEpisodeDetailQuery",
                "读取某一分集的完整内容与场次清单。",
                json!({ "episodeId": { "type": "integer" } }),
                vec!["episodeId"],
            ),
            def(
                "ScriptSceneItemDetailQuery",
                "读取单个场次的详细信息（地点、时间、角色、对白）。",
                json!({ "sceneId": { "type": "integer" } }),
                vec!["sceneId"],
            ),
            def(
                "UpdateScript",
                "更新剧本正文或原始文本。",
                json!({
                    "scriptId": { "type": "integer" },
                    "title": { "type": "string" },
                    "content": { "type": "string" },
                    "rawContent": { "type": "string" },
                    "storySynopsis": { "type": "string" },
                    "summary": { "type": "string" }
                }),
                vec!["scriptId"],
            ),
            def(
                "UpdateScriptInfo",
                "更新剧本元信息（题材、受众、总集数、状态）。",
                json!({
                    "scriptId": { "type": "integer" },
                    "genre": { "type": "string" },
                    "targetAudience": { "type": "string" },
                    "totalEpisodes": { "type": "integer" },
                    "status": { "type": "integer" }
                }),
                vec!["scriptId"],
            ),
            def(
                "SaveScriptEpisode",
                "创建或更新分集。episodeId 为空表示新建。",
                json!({
                    "episodeId": { "type": "integer" },
                    "scriptId": { "type": "integer" },
                    "episodeNumber": { "type": "integer" },
                    "title": { "type": "string" },
                    "synopsis": { "type": "string" },
                    "rawContent": { "type": "string" },
                    "durationEstimate": { "type": "integer" }
                }),
                vec!["scriptId"],
            ),
            def(
                "SaveScriptSceneItems",
                "批量保存场次：按顺序写入某分集下的场次列表（先清空旧场次）。",
                json!({
                    "episodeId": { "type": "integer" },
                    "scenes": { "type": "array", "items": { "type": "object" } }
                }),
                vec!["episodeId", "scenes"],
            ),
            def(
                "ManageScriptSceneItems",
                "对场次做增删改：action = create/update/delete。",
                json!({
                    "action": { "type": "string" },
                    "sceneId": { "type": "integer" },
                    "episodeId": { "type": "integer" },
                    "sceneHeading": { "type": "string" },
                    "location": { "type": "string" },
                    "timeOfDay": { "type": "string" },
                    "intExt": { "type": "string" },
                    "sceneDescription": { "type": "string" }
                }),
                vec!["action"],
            ),
            def(
                "UpdateScriptSceneItem",
                "更新单个场次的字段。",
                json!({
                    "sceneId": { "type": "integer" },
                    "sceneHeading": { "type": "string" },
                    "location": { "type": "string" },
                    "timeOfDay": { "type": "string" },
                    "intExt": { "type": "string" },
                    "sceneDescription": { "type": "string" }
                }),
                vec!["sceneId"],
            ),
            def(
                "DeleteScriptChildResource",
                "删除分集或场次：target = episode/scene。",
                json!({ "target": { "type": "string" }, "id": { "type": "integer" } }),
                vec!["target", "id"],
            ),
            // ---------- 分镜 ----------
            def(
                "GetProjectStoryboard",
                "获取项目分镜容器及其统计信息。",
                json!({ "projectId": { "type": "integer" } }),
                vec!["projectId"],
            ),
            def(
                "StoryboardQuery",
                "查询分镜详情，可按分镜 ID 或项目 ID 定位。",
                json!({ "storyboardId": { "type": "integer" }, "projectId": { "type": "integer" } }),
                vec![],
            ),
            def(
                "GetStoryboardSceneItems",
                "列出分镜中所有镜头条目，可按分集过滤。",
                json!({ "storyboardId": { "type": "integer" }, "episodeId": { "type": "integer" } }),
                vec!["storyboardId"],
            ),
            def(
                "SaveStoryboardEpisode",
                "创建或更新分镜分集。",
                json!({
                    "storyboardEpisodeId": { "type": "integer" },
                    "storyboardId": { "type": "integer" },
                    "scriptEpisodeId": { "type": "integer" },
                    "episodeNumber": { "type": "integer" },
                    "title": { "type": "string" },
                    "synopsis": { "type": "string" }
                }),
                vec!["storyboardId"],
            ),
            def(
                "SaveStoryboardSceneShots",
                "按分集批量写入镜头（自动创建场次并挂载镜头）。",
                json!({
                    "storyboardId": { "type": "integer" },
                    "episodeId": { "type": "integer" },
                    "shots": { "type": "array", "items": { "type": "object" } }
                }),
                vec!["storyboardId", "episodeId", "shots"],
            ),
            def(
                "StoryboardItemInsert",
                "在指定分集末尾插入一个镜头条目。",
                json!({
                    "storyboardId": { "type": "integer" },
                    "episodeId": { "type": "integer" },
                    "sceneId": { "type": "integer" },
                    "content": { "type": "string" },
                    "shotType": { "type": "string" },
                    "duration": { "type": "number" },
                    "cameraMovement": { "type": "string" },
                    "dialogue": { "type": "string" }
                }),
                vec!["storyboardId", "episodeId"],
            ),
            def(
                "UpdateStoryboardItem",
                "更新镜头条目的文本类字段。",
                json!({
                    "itemId": { "type": "integer" },
                    "content": { "type": "string" },
                    "sceneExpectation": { "type": "string" },
                    "dialogue": { "type": "string" },
                    "sound": { "type": "string" },
                    "cameraMovement": { "type": "string" },
                    "shotType": { "type": "string" },
                    "duration": { "type": "number" }
                }),
                vec!["itemId"],
            ),
            def(
                "UpdateStoryboardItemFrame",
                "更新镜头首帧/尾帧图片与提示词。",
                json!({
                    "itemId": { "type": "integer" },
                    "frameType": { "type": "string", "description": "first 或 last" },
                    "imageUrl": { "type": "string" },
                    "prompt": { "type": "string" }
                }),
                vec!["itemId"],
            ),
            def(
                "UpdateStoryboardItemVideo",
                "回填镜头的生成视频地址。",
                json!({
                    "itemId": { "type": "integer" },
                    "videoUrl": { "type": "string" },
                    "videoPrompt": { "type": "string" }
                }),
                vec!["itemId", "videoUrl"],
            ),
            def(
                "DeleteStoryboardChildResource",
                "删除分镜的分集/场次/镜头：target = episode/scene/item。",
                json!({ "target": { "type": "string" }, "id": { "type": "integer" } }),
                vec!["target", "id"],
            ),
            // ---------- 资产 ----------
            def(
                "AssetCreate",
                "创建资产（角色/场景/道具）。",
                json!({
                    "projectId": { "type": "integer" },
                    "type": { "type": "string", "description": "character / scene / prop" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "aiPrompt": { "type": "string" }
                }),
                vec!["projectId", "type", "name"],
            ),
            def(
                "BatchCreateAssets",
                "批量创建资产。",
                json!({
                    "projectId": { "type": "integer" },
                    "assets": { "type": "array", "items": { "type": "object" } }
                }),
                vec!["projectId", "assets"],
            ),
            def(
                "AssetQuery",
                "查询资产详情，可按资产 ID 或 项目+类型 查询。",
                json!({
                    "assetId": { "type": "integer" },
                    "projectId": { "type": "integer" },
                    "type": { "type": "string" },
                    "keyword": { "type": "string" }
                }),
                vec![],
            ),
            def(
                "ListProjectAssets",
                "列出项目的全部资产（含子项）。",
                json!({ "projectId": { "type": "integer" } }),
                vec!["projectId"],
            ),
            def(
                "AssetUpdate",
                "更新资产基础信息。",
                json!({
                    "assetId": { "type": "integer" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "coverUrl": { "type": "string" }
                }),
                vec!["assetId"],
            ),
            def(
                "AssetItemAdd",
                "为资产添加子项（图片变体）。",
                json!({
                    "assetId": { "type": "integer" },
                    "name": { "type": "string" },
                    "imageUrl": { "type": "string" },
                    "aiPrompt": { "type": "string" }
                }),
                vec!["assetId", "imageUrl"],
            ),
            def(
                "BatchCreateAssetItems",
                "批量为资产添加子项。",
                json!({
                    "assetId": { "type": "integer" },
                    "items": { "type": "array", "items": { "type": "object" } }
                }),
                vec!["assetId", "items"],
            ),
            def(
                "QueryAssetItems",
                "查询某个资产的子项列表。",
                json!({ "assetId": { "type": "integer" } }),
                vec!["assetId"],
            ),
            def(
                "QueryAssetMetadata",
                "查询资产类型的字段元数据定义。",
                json!({ "assetType": { "type": "string" } }),
                vec!["assetType"],
            ),
            def(
                "UpdateAssetImage",
                "更新资产封面图。",
                json!({ "assetId": { "type": "integer" }, "coverUrl": { "type": "string" } }),
                vec!["assetId", "coverUrl"],
            ),
            def(
                "UpdateAssetItem",
                "更新资产子项信息。",
                json!({
                    "itemId": { "type": "integer" },
                    "name": { "type": "string" },
                    "imageUrl": { "type": "string" }
                }),
                vec!["itemId"],
            ),
            def(
                "DeleteAssetResource",
                "删除资产或子项：target = asset/item。",
                json!({ "target": { "type": "string" }, "id": { "type": "integer" } }),
                vec!["target", "id"],
            ),
            // ---------- 生成 ----------
            def(
                "GetGenerationModelCapabilities",
                "列出可用的图像/视频模型及其能力约束（尺寸、时长、比例）。",
                json!({ "type": { "type": "integer", "description": "2=图像 3=视频" } }),
                vec![],
            ),
            def(
                "GenerateImage",
                "提交图像生成任务，返回任务 ID；产物稍后可通过任务接口获取。",
                json!({
                    "projectId": { "type": "integer" },
                    "prompt": { "type": "string" },
                    "modelId": { "type": "integer" },
                    "count": { "type": "integer" },
                    "width": { "type": "integer" },
                    "height": { "type": "integer" },
                    "refImageUrls": { "type": "array", "items": { "type": "string" } }
                }),
                vec!["prompt", "modelId"],
            ),
            def(
                "GenerateVideo",
                "提交视频生成任务，返回任务 ID。",
                json!({
                    "projectId": { "type": "integer" },
                    "prompt": { "type": "string" },
                    "modelId": { "type": "integer" },
                    "firstFrameImageUrl": { "type": "string" },
                    "duration": { "type": "integer" },
                    "ratio": { "type": "string" }
                }),
                vec!["prompt", "modelId"],
            ),
            def(
                "DeleteImageTask",
                "删除图像生成任务及其产物记录。",
                json!({ "taskId": { "type": "integer" } }),
                vec!["taskId"],
            ),
            def(
                "DeleteVideoTask",
                "删除视频生成任务及其产物记录。",
                json!({ "taskId": { "type": "integer" } }),
                vec!["taskId"],
            ),
        ]
    }
}

#[async_trait]
impl ToolExecutor for AssistantTools {
    fn definitions(&self) -> Vec<ToolDef> {
        Self::definitions_inner()
    }

    fn requires_confirmation(&self, name: &str) -> bool {
        matches!(
            name,
            "DeleteProject"
                | "DeleteScriptChildResource"
                | "DeleteStoryboardChildResource"
                | "DeleteAssetResource"
                | "DeleteImageTask"
                | "DeleteVideoTask"
        )
    }

    async fn execute(&self, name: &str, args: Value) -> AppResult<String> {
        let state = &self.state;
        match name {
            "ListProjects" => {
                let keyword = format!("%{}%", str_arg(&args, "keyword").unwrap_or_default());
                let rows = sqlx::query(
                    "SELECT id, name, status, update_time FROM afv_project WHERE deleted = 0 AND name LIKE ? ORDER BY update_time DESC LIMIT 100",
                )
                .bind(keyword)
                .fetch_all(&state.db)
                .await?;
                Ok(rows_to_json(rows))
            }
            "ProjectQuery" => {
                let id = int_arg(&args, "projectId")?;
                let row = sqlx::query("SELECT * FROM afv_project WHERE id = ? AND deleted = 0")
                    .bind(id)
                    .fetch_optional(&state.db)
                    .await?;
                Ok(row_to_json(row))
            }
            "SaveProject" => {
                let name = str_arg(&args, "name").unwrap_or_else(|| "未命名项目".into());
                match int_arg_opt(&args, "projectId") {
                    Some(id) => {
                        sqlx::query(
                            "UPDATE afv_project SET name = ?, description = COALESCE(?, description), status = COALESCE(?, status), art_style = COALESCE(?, art_style), art_style_description = COALESCE(?, art_style_description), art_style_image_prompt = COALESCE(?, art_style_image_prompt), update_time = ? WHERE id = ?",
                        )
                        .bind(&name)
                        .bind(str_arg(&args, "description"))
                        .bind(int_arg_opt(&args, "status"))
                        .bind(str_arg(&args, "artStyle"))
                        .bind(str_arg(&args, "artStyleDescription"))
                        .bind(str_arg(&args, "artStyleImagePrompt"))
                        .bind(now())
                        .bind(id)
                        .execute(&state.db)
                        .await?;
                        crate::routes::project::ensure_workspace(state, id, self.user_id).await?;
                        Ok(json!({ "projectId": id, "action": "updated" }).to_string())
                    }
                    None => {
                        let id = sqlx::query(
                            "INSERT INTO afv_project (name, description, scope, owner_type, owner_id, status, art_style, art_style_description, art_style_image_prompt, deleted, create_time, update_time) VALUES (?, ?, 2, 1, ?, 0, ?, ?, ?, 0, ?, ?)",
                        )
                        .bind(&name)
                        .bind(str_arg(&args, "description"))
                        .bind(self.user_id)
                        .bind(str_arg(&args, "artStyle"))
                        .bind(str_arg(&args, "artStyleDescription"))
                        .bind(str_arg(&args, "artStyleImagePrompt"))
                        .bind(now())
                        .bind(now())
                        .execute(&state.db)
                        .await?
                        .last_insert_rowid();
                        crate::routes::project::ensure_workspace(state, id, self.user_id).await?;
                        Ok(json!({ "projectId": id, "action": "created", "name": name }).to_string())
                    }
                }
            }
            "DeleteProject" => {
                let id = int_arg(&args, "projectId")?;
                sqlx::query("UPDATE afv_project SET deleted = 1, update_time = ? WHERE id = ?")
                    .bind(now())
                    .bind(id)
                    .execute(&state.db)
                    .await?;
                Ok(json!({ "deleted": true, "projectId": id }).to_string())
            }
            "GetProjectScript" | "ScriptQuery" => {
                let row = if let Some(id) = int_arg_opt(&args, "scriptId") {
                    sqlx::query("SELECT * FROM afv_script WHERE id = ? AND deleted = 0")
                        .bind(id)
                        .fetch_optional(&state.db)
                        .await?
                } else if let Some(project_id) = int_arg_opt(&args, "projectId") {
                    sqlx::query(
                        "SELECT * FROM afv_script WHERE project_id = ? AND deleted = 0 ORDER BY id ASC LIMIT 1",
                    )
                    .bind(project_id)
                    .fetch_optional(&state.db)
                    .await?
                } else {
                    return Err(AppError::bad_request("需要 scriptId 或 projectId"));
                };
                Ok(row_to_json(row))
            }
            "ScriptStructureQuery" => {
                let script_id = int_arg(&args, "scriptId")?;
                let episodes = sqlx::query(
                    "SELECT id, episode_number, title, synopsis, total_scenes, duration_estimate FROM afv_script_episode WHERE script_id = ? AND deleted = 0 ORDER BY sort_order ASC",
                )
                .bind(script_id)
                .fetch_all(&state.db)
                .await?;
                Ok(json!({ "scriptId": script_id, "episodes": rows_to_value(episodes) }).to_string())
            }
            "ScriptEpisodeDetailQuery" => {
                let id = int_arg(&args, "episodeId")?;
                let episode = sqlx::query("SELECT * FROM afv_script_episode WHERE id = ? AND deleted = 0")
                    .bind(id)
                    .fetch_optional(&state.db)
                    .await?;
                let scenes = sqlx::query(
                    "SELECT * FROM afv_script_scene_item WHERE episode_id = ? AND deleted = 0 ORDER BY sort_order ASC",
                )
                .bind(id)
                .fetch_all(&state.db)
                .await?;
                Ok(json!({
                    "episode": row_to_value(episode),
                    "scenes": rows_to_value(scenes),
                })
                .to_string())
            }
            "ScriptSceneItemDetailQuery" => {
                let id = int_arg(&args, "sceneId")?;
                let row = sqlx::query("SELECT * FROM afv_script_scene_item WHERE id = ? AND deleted = 0")
                    .bind(id)
                    .fetch_optional(&state.db)
                    .await?;
                Ok(row_to_json(row))
            }
            "UpdateScript" => {
                let id = int_arg(&args, "scriptId")?;
                sqlx::query(
                    "UPDATE afv_script SET title = COALESCE(?, title), content = COALESCE(?, content), raw_content = COALESCE(?, raw_content), story_synopsis = COALESCE(?, story_synopsis), summary = COALESCE(?, summary), version = version + 1, update_time = ? WHERE id = ?",
                )
                .bind(str_arg(&args, "title"))
                .bind(str_arg(&args, "content"))
                .bind(str_arg(&args, "rawContent"))
                .bind(str_arg(&args, "storySynopsis"))
                .bind(str_arg(&args, "summary"))
                .bind(now())
                .bind(id)
                .execute(&state.db)
                .await?;
                Ok(json!({ "updated": true, "scriptId": id }).to_string())
            }
            "UpdateScriptInfo" => {
                let id = int_arg(&args, "scriptId")?;
                sqlx::query(
                    "UPDATE afv_script SET genre = COALESCE(?, genre), target_audience = COALESCE(?, target_audience), total_episodes = COALESCE(?, total_episodes), status = COALESCE(?, status), update_time = ? WHERE id = ?",
                )
                .bind(str_arg(&args, "genre"))
                .bind(str_arg(&args, "targetAudience"))
                .bind(int_arg_opt(&args, "totalEpisodes"))
                .bind(int_arg_opt(&args, "status"))
                .bind(now())
                .bind(id)
                .execute(&state.db)
                .await?;
                Ok(json!({ "updated": true, "scriptId": id }).to_string())
            }
            "SaveScriptEpisode" => {
                let script_id = int_arg(&args, "scriptId")?;
                let title = str_arg(&args, "title");
                let synopsis = str_arg(&args, "synopsis");
                let raw = str_arg(&args, "rawContent");
                let duration = int_arg_opt(&args, "durationEstimate");
                match int_arg_opt(&args, "episodeId") {
                    Some(id) => {
                        sqlx::query(
                            "UPDATE afv_script_episode SET title = COALESCE(?, title), synopsis = COALESCE(?, synopsis), raw_content = COALESCE(?, raw_content), duration_estimate = COALESCE(?, duration_estimate), version = version + 1, update_time = ? WHERE id = ?",
                        )
                        .bind(title)
                        .bind(synopsis)
                        .bind(raw)
                        .bind(duration)
                        .bind(now())
                        .bind(id)
                        .execute(&state.db)
                        .await?;
                        Ok(json!({ "episodeId": id, "action": "updated" }).to_string())
                    }
                    None => {
                        let number = match int_arg_opt(&args, "episodeNumber") {
                            Some(n) => n,
                            None => sqlx::query_scalar::<_, i64>(
                                "SELECT IFNULL(MAX(episode_number), 0) + 1 FROM afv_script_episode WHERE script_id = ? AND deleted = 0",
                            )
                            .bind(script_id)
                            .fetch_one(&state.db)
                            .await?,
                        };
                        let id = sqlx::query(
                            "INSERT INTO afv_script_episode (script_id, episode_number, title, synopsis, raw_content, duration_estimate, total_scenes, source_type, sort_order, parsing_status, status, version, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?, 0, 0, 0, 0, ?, ?)",
                        )
                        .bind(script_id)
                        .bind(number)
                        .bind(title)
                        .bind(synopsis)
                        .bind(raw)
                        .bind(duration)
                        .bind(number)
                        .bind(now())
                        .bind(now())
                        .execute(&state.db)
                        .await?
                        .last_insert_rowid();
                        update_script_episode_count(state, script_id).await?;
                        Ok(json!({ "episodeId": id, "action": "created", "episodeNumber": number }).to_string())
                    }
                }
            }
            "SaveScriptSceneItems" => {
                let episode_id = int_arg(&args, "episodeId")?;
                let scenes = args
                    .get("scenes")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let script_id: i64 =
                    sqlx::query_scalar("SELECT script_id FROM afv_script_episode WHERE id = ?")
                        .bind(episode_id)
                        .fetch_one(&state.db)
                        .await?;
                sqlx::query(
                    "UPDATE afv_script_scene_item SET deleted = 1, update_time = ? WHERE episode_id = ?",
                )
                .bind(now())
                .bind(episode_id)
                .execute(&state.db)
                .await?;

                let mut created = 0;
                for (idx, scene) in scenes.iter().enumerate() {
                    sqlx::query(
                        "INSERT INTO afv_script_scene_item (episode_id, script_id, scene_number, scene_heading, location, time_of_day, int_ext, characters, scene_description, dialogues, sort_order, status, version, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, 0, ?, ?)",
                    )
                    .bind(episode_id)
                    .bind(script_id)
                    .bind(str_arg(scene, "sceneNumber"))
                    .bind(str_arg(scene, "sceneHeading"))
                    .bind(str_arg(scene, "location"))
                    .bind(str_arg(scene, "timeOfDay"))
                    .bind(str_arg(scene, "intExt"))
                    .bind(scene.get("characters").map(|v| v.to_string()))
                    .bind(str_arg(scene, "sceneDescription"))
                    .bind(scene.get("dialogues").map(|v| v.to_string()))
                    .bind(idx as i64 + 1)
                    .bind(now())
                    .bind(now())
                    .execute(&state.db)
                    .await?;
                    created += 1;
                }
                let _ = sqlx::query("UPDATE afv_script_episode SET total_scenes = ?, update_time = ? WHERE id = ?")
                    .bind(created)
                    .bind(now())
                    .bind(episode_id)
                    .execute(&state.db)
                    .await;
                Ok(json!({ "episodeId": episode_id, "scenes": created }).to_string())
            }
            "ManageScriptSceneItems" => {
                let action = str_arg(&args, "action").unwrap_or_else(|| "create".into());
                match action.as_str() {
                    "delete" => {
                        let id = int_arg(&args, "sceneId")?;
                        sqlx::query("UPDATE afv_script_scene_item SET deleted = 1, update_time = ? WHERE id = ?")
                            .bind(now())
                            .bind(id)
                            .execute(&state.db)
                            .await?;
                        Ok(json!({ "deleted": true, "sceneId": id }).to_string())
                    }
                    "update" => self.execute("UpdateScriptSceneItem", args).await,
                    _ => {
                        let episode_id = int_arg(&args, "episodeId")?;
                        let script_id: i64 =
                            sqlx::query_scalar("SELECT script_id FROM afv_script_episode WHERE id = ?")
                                .bind(episode_id)
                                .fetch_one(&state.db)
                                .await?;
                        let order: i64 = sqlx::query_scalar(
                            "SELECT IFNULL(MAX(sort_order), 0) + 1 FROM afv_script_scene_item WHERE episode_id = ? AND deleted = 0",
                        )
                        .bind(episode_id)
                        .fetch_one(&state.db)
                        .await?;
                        let id = sqlx::query(
                            "INSERT INTO afv_script_scene_item (episode_id, script_id, scene_heading, location, time_of_day, int_ext, scene_description, sort_order, status, version, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 0, 0, ?, ?)",
                        )
                        .bind(episode_id)
                        .bind(script_id)
                        .bind(str_arg(&args, "sceneHeading"))
                        .bind(str_arg(&args, "location"))
                        .bind(str_arg(&args, "timeOfDay"))
                        .bind(str_arg(&args, "intExt"))
                        .bind(str_arg(&args, "sceneDescription"))
                        .bind(order)
                        .bind(now())
                        .bind(now())
                        .execute(&state.db)
                        .await?
                        .last_insert_rowid();
                        Ok(json!({ "sceneId": id, "action": "created" }).to_string())
                    }
                }
            }
            "UpdateScriptSceneItem" => {
                let id = int_arg(&args, "sceneId")?;
                sqlx::query(
                    "UPDATE afv_script_scene_item SET scene_heading = COALESCE(?, scene_heading), location = COALESCE(?, location), time_of_day = COALESCE(?, time_of_day), int_ext = COALESCE(?, int_ext), scene_description = COALESCE(?, scene_description), version = version + 1, update_time = ? WHERE id = ?",
                )
                .bind(str_arg(&args, "sceneHeading"))
                .bind(str_arg(&args, "location"))
                .bind(str_arg(&args, "timeOfDay"))
                .bind(str_arg(&args, "intExt"))
                .bind(str_arg(&args, "sceneDescription"))
                .bind(now())
                .bind(id)
                .execute(&state.db)
                .await?;
                Ok(json!({ "updated": true, "sceneId": id }).to_string())
            }
            "DeleteScriptChildResource" => {
                let target = str_arg(&args, "target").unwrap_or_default();
                let id = int_arg(&args, "id")?;
                let table = match target.as_str() {
                    "episode" => "afv_script_episode",
                    "scene" => "afv_script_scene_item",
                    _ => return Err(AppError::bad_request("target 只能是 episode 或 scene")),
                };
                let sql = format!("UPDATE {table} SET deleted = 1, update_time = ? WHERE id = ?");
                sqlx::query(&sql).bind(now()).bind(id).execute(&state.db).await?;
                Ok(json!({ "deleted": true, "target": target, "id": id }).to_string())
            }
            "GetProjectStoryboard" | "StoryboardQuery" => {
                let row = if let Some(id) = int_arg_opt(&args, "storyboardId") {
                    sqlx::query("SELECT * FROM afv_storyboard WHERE id = ? AND deleted = 0")
                        .bind(id)
                        .fetch_optional(&state.db)
                        .await?
                } else if let Some(project_id) = int_arg_opt(&args, "projectId") {
                    sqlx::query(
                        "SELECT * FROM afv_storyboard WHERE project_id = ? AND deleted = 0 ORDER BY id ASC LIMIT 1",
                    )
                    .bind(project_id)
                    .fetch_optional(&state.db)
                    .await?
                } else {
                    return Err(AppError::bad_request("需要 storyboardId 或 projectId"));
                };
                Ok(row_to_json(row))
            }
            "GetStoryboardSceneItems" => {
                let storyboard_id = int_arg(&args, "storyboardId")?;
                let rows = match int_arg_opt(&args, "episodeId") {
                    Some(episode_id) => {
                        sqlx::query(
                            "SELECT * FROM afv_storyboard_item WHERE storyboard_id = ? AND storyboard_episode_id = ? AND deleted = 0 ORDER BY sort_order ASC",
                        )
                        .bind(storyboard_id)
                        .bind(episode_id)
                        .fetch_all(&state.db)
                        .await?
                    }
                    None => {
                        sqlx::query(
                            "SELECT * FROM afv_storyboard_item WHERE storyboard_id = ? AND deleted = 0 ORDER BY storyboard_episode_id ASC, sort_order ASC",
                        )
                        .bind(storyboard_id)
                        .fetch_all(&state.db)
                        .await?
                    }
                };
                Ok(rows_to_json(rows))
            }
            "SaveStoryboardEpisode" => {
                let storyboard_id = int_arg(&args, "storyboardId")?;
                let title = str_arg(&args, "title");
                let synopsis = str_arg(&args, "synopsis");
                match int_arg_opt(&args, "storyboardEpisodeId") {
                    Some(id) => {
                        sqlx::query(
                            "UPDATE afv_storyboard_episode SET title = COALESCE(?, title), synopsis = COALESCE(?, synopsis), script_episode_id = COALESCE(?, script_episode_id), update_time = ? WHERE id = ?",
                        )
                        .bind(title)
                        .bind(synopsis)
                        .bind(int_arg_opt(&args, "scriptEpisodeId"))
                        .bind(now())
                        .bind(id)
                        .execute(&state.db)
                        .await?;
                        Ok(json!({ "storyboardEpisodeId": id, "action": "updated" }).to_string())
                    }
                    None => {
                        let number = match int_arg_opt(&args, "episodeNumber") {
                            Some(n) => n,
                            None => sqlx::query_scalar::<_, i64>(
                                "SELECT IFNULL(MAX(episode_number), 0) + 1 FROM afv_storyboard_episode WHERE storyboard_id = ? AND deleted = 0",
                            )
                            .bind(storyboard_id)
                            .fetch_one(&state.db)
                            .await?,
                        };
                        let id = sqlx::query(
                            "INSERT INTO afv_storyboard_episode (storyboard_id, script_episode_id, episode_number, title, synopsis, sort_order, status, deleted, create_time, update_time, compose_status) VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?, ?, 0)",
                        )
                        .bind(storyboard_id)
                        .bind(int_arg_opt(&args, "scriptEpisodeId"))
                        .bind(number)
                        .bind(title)
                        .bind(synopsis)
                        .bind(number)
                        .bind(now())
                        .bind(now())
                        .execute(&state.db)
                        .await?
                        .last_insert_rowid();
                        Ok(json!({ "storyboardEpisodeId": id, "action": "created", "episodeNumber": number }).to_string())
                    }
                }
            }
            "SaveStoryboardSceneShots" => {
                let storyboard_id = int_arg(&args, "storyboardId")?;
                let episode_id = int_arg(&args, "episodeId")?;
                let shots = args
                    .get("shots")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                sqlx::query(
                    "UPDATE afv_storyboard_item SET deleted = 1, update_time = ? WHERE storyboard_episode_id = ?",
                )
                .bind(now())
                .bind(episode_id)
                .execute(&state.db)
                .await?;

                let mut created = 0;
                let mut current_scene: Option<i64> = None;
                for (idx, shot) in shots.iter().enumerate() {
                    if let Some(heading) = str_arg(shot, "sceneHeading") {
                        current_scene = Some(
                            sqlx::query(
                                "INSERT INTO afv_storyboard_scene (episode_id, storyboard_id, scene_number, scene_heading, location, time_of_day, int_ext, sort_order, status, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)",
                            )
                            .bind(episode_id)
                            .bind(storyboard_id)
                            .bind(str_arg(shot, "sceneNumber"))
                            .bind(&heading)
                            .bind(str_arg(shot, "location"))
                            .bind(str_arg(shot, "timeOfDay"))
                            .bind(str_arg(shot, "intExt"))
                            .bind(created as i64 + 1)
                            .bind(now())
                            .bind(now())
                            .execute(&state.db)
                            .await?
                            .last_insert_rowid(),
                        );
                    }

                    sqlx::query(
                        "INSERT INTO afv_storyboard_item (storyboard_id, storyboard_episode_id, storyboard_scene_id, sort_order, shot_number, shot_type, duration, content, scene_expectation, dialogue, sound, camera_movement, camera_angle, transition, video_prompt, ai_generated, status, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 0, 0, ?, ?)",
                    )
                    .bind(storyboard_id)
                    .bind(episode_id)
                    .bind(current_scene)
                    .bind(idx as i64 + 1)
                    .bind(str_arg(shot, "shotNumber"))
                    .bind(str_arg(shot, "shotType"))
                    .bind(shot.get("duration").and_then(|v| v.as_f64()))
                    .bind(str_arg(shot, "content"))
                    .bind(str_arg(shot, "sceneExpectation"))
                    .bind(str_arg(shot, "dialogue"))
                    .bind(str_arg(shot, "sound"))
                    .bind(str_arg(shot, "cameraMovement"))
                    .bind(str_arg(shot, "cameraAngle"))
                    .bind(str_arg(shot, "transition"))
                    .bind(str_arg(shot, "videoPrompt"))
                    .bind(now())
                    .bind(now())
                    .execute(&state.db)
                    .await?;
                    created += 1;
                }
                Ok(json!({ "storyboardId": storyboard_id, "episodeId": episode_id, "shots": created }).to_string())
            }
            "StoryboardItemInsert" => {
                let storyboard_id = int_arg(&args, "storyboardId")?;
                let episode_id = int_arg(&args, "episodeId")?;
                let order: i64 = sqlx::query_scalar(
                    "SELECT IFNULL(MAX(sort_order), 0) + 1 FROM afv_storyboard_item WHERE storyboard_episode_id = ? AND deleted = 0",
                )
                .bind(episode_id)
                .fetch_one(&state.db)
                .await?;
                let id = sqlx::query(
                    "INSERT INTO afv_storyboard_item (storyboard_id, storyboard_episode_id, storyboard_scene_id, sort_order, content, shot_type, duration, camera_movement, dialogue, ai_generated, status, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 0, 0, ?, ?)",
                )
                .bind(storyboard_id)
                .bind(episode_id)
                .bind(int_arg_opt(&args, "sceneId"))
                .bind(order)
                .bind(str_arg(&args, "content"))
                .bind(str_arg(&args, "shotType"))
                .bind(args.get("duration").and_then(|v| v.as_f64()))
                .bind(str_arg(&args, "cameraMovement"))
                .bind(str_arg(&args, "dialogue"))
                .bind(now())
                .bind(now())
                .execute(&state.db)
                .await?
                .last_insert_rowid();
                Ok(json!({ "itemId": id, "action": "created" }).to_string())
            }
            "UpdateStoryboardItem" => {
                let id = int_arg(&args, "itemId")?;
                sqlx::query(
                    "UPDATE afv_storyboard_item SET content = COALESCE(?, content), scene_expectation = COALESCE(?, scene_expectation), dialogue = COALESCE(?, dialogue), sound = COALESCE(?, sound), camera_movement = COALESCE(?, camera_movement), shot_type = COALESCE(?, shot_type), duration = COALESCE(?, duration), update_time = ? WHERE id = ?",
                )
                .bind(str_arg(&args, "content"))
                .bind(str_arg(&args, "sceneExpectation"))
                .bind(str_arg(&args, "dialogue"))
                .bind(str_arg(&args, "sound"))
                .bind(str_arg(&args, "cameraMovement"))
                .bind(str_arg(&args, "shotType"))
                .bind(args.get("duration").and_then(|v| v.as_f64()))
                .bind(now())
                .bind(id)
                .execute(&state.db)
                .await?;
                Ok(json!({ "updated": true, "itemId": id }).to_string())
            }
            "UpdateStoryboardItemFrame" => {
                let id = int_arg(&args, "itemId")?;
                let frame_type = str_arg(&args, "frameType").unwrap_or_else(|| "first".into());
                if frame_type == "last" {
                    sqlx::query(
                        "UPDATE afv_storyboard_item SET last_frame_image_url = COALESCE(?, last_frame_image_url), last_frame_prompt = COALESCE(?, last_frame_prompt), update_time = ? WHERE id = ?",
                    )
                    .bind(str_arg(&args, "imageUrl"))
                    .bind(str_arg(&args, "prompt"))
                    .bind(now())
                    .bind(id)
                    .execute(&state.db)
                    .await?;
                } else {
                    sqlx::query(
                        "UPDATE afv_storyboard_item SET first_frame_image_url = COALESCE(?, first_frame_image_url), first_frame_prompt = COALESCE(?, first_frame_prompt), update_time = ? WHERE id = ?",
                    )
                    .bind(str_arg(&args, "imageUrl"))
                    .bind(str_arg(&args, "prompt"))
                    .bind(now())
                    .bind(id)
                    .execute(&state.db)
                    .await?;
                }
                Ok(json!({ "updated": true, "itemId": id, "frameType": frame_type }).to_string())
            }
            "UpdateStoryboardItemVideo" => {
                let id = int_arg(&args, "itemId")?;
                sqlx::query(
                    "UPDATE afv_storyboard_item SET generated_video_url = ?, video_prompt = COALESCE(?, video_prompt), update_time = ? WHERE id = ?",
                )
                .bind(str_arg(&args, "videoUrl"))
                .bind(str_arg(&args, "videoPrompt"))
                .bind(now())
                .bind(id)
                .execute(&state.db)
                .await?;
                Ok(json!({ "updated": true, "itemId": id }).to_string())
            }
            "DeleteStoryboardChildResource" => {
                let target = str_arg(&args, "target").unwrap_or_default();
                let id = int_arg(&args, "id")?;
                let table = match target.as_str() {
                    "episode" => "afv_storyboard_episode",
                    "scene" => "afv_storyboard_scene",
                    "item" => "afv_storyboard_item",
                    _ => return Err(AppError::bad_request("target 只能是 episode/scene/item")),
                };
                let sql = format!("UPDATE {table} SET deleted = 1, update_time = ? WHERE id = ?");
                sqlx::query(&sql).bind(now()).bind(id).execute(&state.db).await?;
                Ok(json!({ "deleted": true, "target": target, "id": id }).to_string())
            }
            "AssetCreate" | "BatchCreateAssets" => {
                let project_id = int_arg(&args, "projectId")?;
                let assets: Vec<Value> = if name == "BatchCreateAssets" {
                    args.get("assets").and_then(|v| v.as_array()).cloned().unwrap_or_default()
                } else {
                    vec![args.clone()]
                };
                let mut ids = Vec::new();
                for asset in assets {
                    let asset_type = str_arg(&asset, "type").unwrap_or_else(|| "character".into());
                    let name_value = str_arg(&asset, "name").unwrap_or_else(|| "未命名资产".into());
                    let id = sqlx::query(
                        "INSERT INTO afv_asset (user_id, project_id, type, name, description, ai_prompt, source_type, owner_type, owner_id, status, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, 0, 1, ?, 0, 0, ?, ?)",
                    )
                    .bind(self.user_id)
                    .bind(project_id)
                    .bind(&asset_type)
                    .bind(&name_value)
                    .bind(str_arg(&asset, "description"))
                    .bind(str_arg(&asset, "aiPrompt"))
                    .bind(self.user_id)
                    .bind(now())
                    .bind(now())
                    .execute(&state.db)
                    .await?
                    .last_insert_rowid();
                    ids.push(id);
                }
                Ok(json!({ "projectId": project_id, "assetIds": ids }).to_string())
            }
            "AssetQuery" => {
                if let Some(id) = int_arg_opt(&args, "assetId") {
                    let row = sqlx::query("SELECT * FROM afv_asset WHERE id = ? AND deleted = 0")
                        .bind(id)
                        .fetch_optional(&state.db)
                        .await?;
                    return Ok(row_to_json(row));
                }
                let project_id = int_arg_opt(&args, "projectId");
                let asset_type = str_arg(&args, "type");
                let keyword = format!("%{}%", str_arg(&args, "keyword").unwrap_or_default());
                let rows = sqlx::query(
                    "SELECT * FROM afv_asset WHERE deleted = 0 AND (? IS NULL OR project_id = ?) AND (? IS NULL OR type = ?) AND name LIKE ? ORDER BY id ASC",
                )
                .bind(project_id)
                .bind(project_id)
                .bind(asset_type.clone())
                .bind(asset_type)
                .bind(keyword)
                .fetch_all(&state.db)
                .await?;
                Ok(rows_to_json(rows))
            }
            "ListProjectAssets" => {
                let project_id = int_arg(&args, "projectId")?;
                let assets = sqlx::query(
                    "SELECT id, type, name, description, cover_url FROM afv_asset WHERE project_id = ? AND deleted = 0 ORDER BY type ASC, id ASC",
                )
                .bind(project_id)
                .fetch_all(&state.db)
                .await?;
                let mut result = Vec::new();
                for asset in assets {
                    let id: i64 = asset.get("id");
                    let items = sqlx::query(
                        "SELECT id, name, image_url FROM afv_asset_item WHERE asset_id = ? AND deleted = 0 ORDER BY sort_order ASC",
                    )
                    .bind(id)
                    .fetch_all(&state.db)
                    .await?;
                    let mut value = row_to_value(Some(asset));
                    if let Some(map) = value.as_object_mut() {
                        map.insert("items".into(), rows_to_value(items));
                    }
                    result.push(value);
                }
                Ok(Value::Array(result).to_string())
            }
            "AssetUpdate" => {
                let id = int_arg(&args, "assetId")?;
                sqlx::query(
                    "UPDATE afv_asset SET name = COALESCE(?, name), description = COALESCE(?, description), cover_url = COALESCE(?, cover_url), update_time = ? WHERE id = ?",
                )
                .bind(str_arg(&args, "name"))
                .bind(str_arg(&args, "description"))
                .bind(str_arg(&args, "coverUrl"))
                .bind(now())
                .bind(id)
                .execute(&state.db)
                .await?;
                Ok(json!({ "updated": true, "assetId": id }).to_string())
            }
            "AssetItemAdd" | "BatchCreateAssetItems" => {
                let asset_id = int_arg(&args, "assetId")?;
                let items: Vec<Value> = if name == "BatchCreateAssetItems" {
                    args.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default()
                } else {
                    vec![args.clone()]
                };
                let mut ids = Vec::new();
                for item in items {
                    let order: i64 = sqlx::query_scalar(
                        "SELECT IFNULL(MAX(sort_order), 0) + 1 FROM afv_asset_item WHERE asset_id = ? AND deleted = 0",
                    )
                    .bind(asset_id)
                    .fetch_one(&state.db)
                    .await?;
                    let id = sqlx::query(
                        "INSERT INTO afv_asset_item (asset_id, item_type, name, image_url, ai_prompt, sort_order, source_type, deleted, create_time, update_time) VALUES (?, 'image', ?, ?, ?, ?, 0, 0, ?, ?)",
                    )
                    .bind(asset_id)
                    .bind(str_arg(&item, "name"))
                    .bind(str_arg(&item, "imageUrl").unwrap_or_default())
                    .bind(str_arg(&item, "aiPrompt"))
                    .bind(order)
                    .bind(now())
                    .bind(now())
                    .execute(&state.db)
                    .await?
                    .last_insert_rowid();
                    ids.push(id);
                }
                Ok(json!({ "assetId": asset_id, "itemIds": ids }).to_string())
            }
            "QueryAssetItems" => {
                let asset_id = int_arg(&args, "assetId")?;
                let rows = sqlx::query(
                    "SELECT * FROM afv_asset_item WHERE asset_id = ? AND deleted = 0 ORDER BY sort_order ASC",
                )
                .bind(asset_id)
                .fetch_all(&state.db)
                .await?;
                Ok(rows_to_json(rows))
            }
            "QueryAssetMetadata" => {
                let asset_type = str_arg(&args, "assetType").unwrap_or_else(|| "character".into());
                Ok(json!({
                    "assetType": asset_type,
                    "fields": ["gender", "age", "personality", "appearance"]
                })
                .to_string())
            }
            "UpdateAssetImage" => {
                let id = int_arg(&args, "assetId")?;
                let url = str_arg(&args, "coverUrl").unwrap_or_default();
                sqlx::query("UPDATE afv_asset SET cover_url = ?, update_time = ? WHERE id = ?")
                    .bind(url)
                    .bind(now())
                    .bind(id)
                    .execute(&state.db)
                    .await?;
                Ok(json!({ "updated": true, "assetId": id }).to_string())
            }
            "UpdateAssetItem" => {
                let id = int_arg(&args, "itemId")?;
                sqlx::query(
                    "UPDATE afv_asset_item SET name = COALESCE(?, name), image_url = COALESCE(?, image_url), update_time = ? WHERE id = ?",
                )
                .bind(str_arg(&args, "name"))
                .bind(str_arg(&args, "imageUrl"))
                .bind(now())
                .bind(id)
                .execute(&state.db)
                .await?;
                Ok(json!({ "updated": true, "itemId": id }).to_string())
            }
            "DeleteAssetResource" => {
                let target = str_arg(&args, "target").unwrap_or_else(|| "asset".into());
                let id = int_arg(&args, "id")?;
                let table = if target == "item" { "afv_asset_item" } else { "afv_asset" };
                let sql = format!("UPDATE {table} SET deleted = 1, update_time = ? WHERE id = ?");
                sqlx::query(&sql).bind(now()).bind(id).execute(&state.db).await?;
                Ok(json!({ "deleted": true, "target": target, "id": id }).to_string())
            }
            "GetGenerationModelCapabilities" => {
                let model_type = int_arg_opt(&args, "type");
                let rows = sqlx::query(
                    "SELECT id, name, code, model_type, config, max_concurrency FROM afv_ai_model WHERE deleted = 0 AND status = 1 AND (? IS NULL OR model_type = ?) ORDER BY sort ASC",
                )
                .bind(model_type)
                .bind(model_type)
                .fetch_all(&state.db)
                .await?;
                Ok(rows_to_json(rows))
            }
            "GenerateImage" => {
                let model_id = int_arg(&args, "modelId")?;
                let prompt = str_arg(&args, "prompt").unwrap_or_default();
                let task_id = crate::support::new_task_id("img");
                let id = sqlx::query(
                    "INSERT INTO afv_image_task (task_id, user_id, project_id, prompt, count, success_count, status, model_id, owner_type, owner_id, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, 0, 0, ?, 1, ?, 0, ?, ?)",
                )
                .bind(&task_id)
                .bind(self.user_id)
                .bind(int_arg_opt(&args, "projectId"))
                .bind(&prompt)
                .bind(int_arg_opt(&args, "count").unwrap_or(1))
                .bind(model_id)
                .bind(self.user_id)
                .bind(now())
                .bind(now())
                .execute(&state.db)
                .await?
                .last_insert_rowid();

                let state = self.state.clone();
                let width = int_arg_opt(&args, "width");
                let height = int_arg_opt(&args, "height");
                let refs = args
                    .get("refImageUrls")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let count = int_arg_opt(&args, "count").unwrap_or(1);
                tokio::spawn(async move {
                    crate::routes::generation::run_image_task_public(
                        state, id, prompt, refs, width, height, count,
                    )
                    .await;
                });
                Ok(json!({ "taskId": id, "businessTaskId": task_id, "status": "queued" }).to_string())
            }
            "GenerateVideo" => {
                let model_id = int_arg(&args, "modelId")?;
                let prompt = str_arg(&args, "prompt").unwrap_or_default();
                let task_id = crate::support::new_task_id("vid");
                let id = sqlx::query(
                    "INSERT INTO afv_video_task (task_id, user_id, project_id, prompt, first_frame_image_url, duration, ratio, count, success_count, status, model_id, owner_type, owner_id, deleted, create_time, update_time) VALUES (?, ?, ?, ?, ?, ?, ?, 1, 0, 0, ?, 1, ?, 0, ?, ?)",
                )
                .bind(&task_id)
                .bind(self.user_id)
                .bind(int_arg_opt(&args, "projectId"))
                .bind(&prompt)
                .bind(str_arg(&args, "firstFrameImageUrl"))
                .bind(int_arg_opt(&args, "duration"))
                .bind(str_arg(&args, "ratio"))
                .bind(model_id)
                .bind(self.user_id)
                .bind(now())
                .bind(now())
                .execute(&state.db)
                .await?
                .last_insert_rowid();

                let state = self.state.clone();
                let first_frame = str_arg(&args, "firstFrameImageUrl");
                let ratio = str_arg(&args, "ratio");
                let duration = int_arg_opt(&args, "duration");
                tokio::spawn(async move {
                    crate::routes::generation::run_video_task_public(
                        state, id, prompt, first_frame, ratio, duration,
                    )
                    .await;
                });
                Ok(json!({ "taskId": id, "businessTaskId": task_id, "status": "queued" }).to_string())
            }
            "DeleteImageTask" => {
                let id = int_arg(&args, "taskId")?;
                sqlx::query("UPDATE afv_image_task SET deleted = 1, update_time = ? WHERE id = ?")
                    .bind(now())
                    .bind(id)
                    .execute(&state.db)
                    .await?;
                Ok(json!({ "deleted": true, "taskId": id }).to_string())
            }
            "DeleteVideoTask" => {
                let id = int_arg(&args, "taskId")?;
                sqlx::query("UPDATE afv_video_task SET deleted = 1, update_time = ? WHERE id = ?")
                    .bind(now())
                    .bind(id)
                    .execute(&state.db)
                    .await?;
                Ok(json!({ "deleted": true, "taskId": id }).to_string())
            }
            other => Err(AppError::bad_request(format!("未知工具: {other}"))),
        }
    }
}

async fn update_script_episode_count(state: &AppState, script_id: i64) -> AppResult<()> {
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(1) FROM afv_script_episode WHERE script_id = ? AND deleted = 0")
            .bind(script_id)
            .fetch_one(&state.db)
            .await?;
    sqlx::query("UPDATE afv_script SET total_episodes = ?, update_time = ? WHERE id = ?")
        .bind(total)
        .bind(now())
        .bind(script_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

fn str_arg(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn int_arg(value: &Value, key: &str) -> AppResult<i64> {
    int_arg_opt(value, key).ok_or_else(|| AppError::bad_request(format!("缺少参数 {key}")))
}

fn int_arg_opt(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
    })
}

fn row_to_value(row: Option<sqlx::sqlite::SqliteRow>) -> Value {
    match row {
        Some(row) => {
            let mut map = serde_json::Map::new();
            for (idx, column) in row.columns().iter().enumerate() {
                let name = column.name().to_string();
                let value = sqlite_value(&row, idx);
                map.insert(name, value);
            }
            Value::Object(map)
        }
        None => Value::Null,
    }
}

fn row_to_json(row: Option<sqlx::sqlite::SqliteRow>) -> String {
    row_to_value(row).to_string()
}

fn rows_to_value(rows: Vec<sqlx::sqlite::SqliteRow>) -> Value {
    Value::Array(rows.into_iter().map(|r| row_to_value(Some(r))).collect())
}

fn rows_to_json(rows: Vec<sqlx::sqlite::SqliteRow>) -> String {
    rows_to_value(rows).to_string()
}

fn sqlite_value(row: &sqlx::sqlite::SqliteRow, idx: usize) -> Value {
    use sqlx::{Row, TypeInfo, ValueRef};
    let raw = row.try_get_raw(idx);
    let Ok(raw) = raw else { return Value::Null };
    if raw.is_null() {
        return Value::Null;
    }
    let type_name = raw.type_info().name().to_string();
    match type_name.as_str() {
        "INTEGER" | "INT" | "BIGINT" => row
            .try_get::<i64, _>(idx)
            .map(Value::from)
            .unwrap_or(Value::Null),
        "REAL" | "DOUBLE" | "FLOAT" => row
            .try_get::<f64, _>(idx)
            .map(|v| Value::from(v))
            .unwrap_or(Value::Null),
        _ => row
            .try_get::<String, _>(idx)
            .map(Value::String)
            .unwrap_or_else(|_| {
                row.try_get::<i64, _>(idx)
                    .map(Value::from)
                    .unwrap_or(Value::Null)
            }),
    }
}

