//! 项目 Profile 数据访问对象
//!
//! profiles 表存放全应用共享的项目实体（供应商/MCP/Skills/Prompt 快照），
//! payload 为原始 JSON 文本（按 app 分槽），解析在 service 层进行。
//! 各应用分组（scope）独立的 current 标记存放于 settings 表（key-value）。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;

/// 每个 scope 的 current 标记 key = 前缀 + scope（如 current_profile_id_claude）
const CURRENT_PROFILE_ID_KEY_PREFIX: &str = "current_profile_id_";

fn current_profile_key(scope: &str) -> String {
    format!("{CURRENT_PROFILE_ID_KEY_PREFIX}{scope}")
}

/// 项目 Profile 记录（全应用共享，无所属分组）
#[derive(Debug, Clone)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// 原始 JSON 快照文本（ProfilePayload），解析在 service 层
    pub payload: String,
    pub sort_order: Option<i64>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl Database {
    /// 获取所有项目（按 sort_order 优先、created_at 兜底排序）
    pub fn get_all_profiles(&self) -> Result<Vec<Profile>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, payload, sort_order, created_at, updated_at
                 FROM profiles
                 ORDER BY sort_order IS NULL, sort_order, created_at, id",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Profile {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    payload: row.get(2)?,
                    sort_order: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut profiles = Vec::new();
        for row in rows {
            profiles.push(row.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(profiles)
    }

    /// 获取单个项目
    pub fn get_profile(&self, id: &str) -> Result<Option<Profile>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, payload, sort_order, created_at, updated_at
                 FROM profiles WHERE id = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        match stmt.query_row(params![id], |row| {
            Ok(Profile {
                id: row.get(0)?,
                name: row.get(1)?,
                payload: row.get(2)?,
                sort_order: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        }) {
            Ok(profile) => Ok(Some(profile)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 保存项目（插入或整行替换）
    pub fn save_profile(&self, profile: &Profile) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let duplicate: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM profiles WHERE name = ?1 AND id <> ?2)
                 AND NOT EXISTS(SELECT 1 FROM profiles WHERE id = ?2 AND name = ?1)",
                params![profile.name.trim(), profile.id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if duplicate {
            return Err(AppError::Message("profiles.nameExists".into()));
        }
        conn.execute(
            "INSERT OR REPLACE INTO profiles
             (id, name, payload, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                profile.id,
                profile.name.trim(),
                profile.payload,
                profile.sort_order,
                profile.created_at,
                profile.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除项目，返回是否实际删除了记录
    pub fn delete_profile(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute("DELETE FROM profiles WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    /// 读取某分组当前激活的项目 id（未使用项目时为 None）
    pub fn get_current_profile_id(&self, scope: &str) -> Result<Option<String>, AppError> {
        self.get_setting(&current_profile_key(scope))
    }

    /// 设置某分组当前激活的项目 id；None 表示"不使用项目"（删除 key）
    pub fn set_current_profile_id(&self, scope: &str, id: Option<&str>) -> Result<(), AppError> {
        let key = current_profile_key(scope);
        match id {
            Some(id) => self.set_setting(&key, id),
            None => {
                let conn = lock_conn!(self.conn);
                conn.execute("DELETE FROM settings WHERE key = ?1", params![key])
                    .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str, name: &str, sort_order: Option<i64>) -> Profile {
        Profile {
            id: id.to_string(),
            name: name.to_string(),
            payload: r#"{"providers":{"claude":null,"codex":null}}"#.to_string(),
            sort_order,
            created_at: Some(1_000),
            updated_at: Some(1_000),
        }
    }

    #[test]
    fn test_profile_names_must_be_unique_for_create_and_rename() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.save_profile(&sample("a", "Work", None))?;
        db.save_profile(&sample("b", "Personal", None))?;
        let duplicate = db.save_profile(&sample("c", "  Work  ", None));
        assert!(
            matches!(duplicate, Err(AppError::Message(ref key)) if key == "profiles.nameExists")
        );
        assert!(db.get_profile("c")?.is_none());
        let rename = db.save_profile(&sample("b", "Work", None));
        assert!(matches!(rename, Err(AppError::Message(ref key)) if key == "profiles.nameExists"));
        assert_eq!(
            db.get_profile("b")?.map(|p| p.name),
            Some("Personal".into())
        );
        db.save_profile(&sample("a", "Work", None))?;
        db.save_profile(&sample("a", "Renamed", None))?;
        db.save_profile(&sample("c", "Work", None))?;
        Ok(())
    }

    #[test]
    fn test_existing_duplicate_profiles_can_still_refresh_or_be_renamed() -> Result<(), AppError> {
        let db = Database::memory()?;
        db.save_profile(&sample("a", "Work", None))?;
        db.save_profile(&sample("b", "Personal", None))?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute("UPDATE profiles SET name = 'Work' WHERE id = 'b'", [])
                .map_err(|e| AppError::Database(e.to_string()))?;
        }
        let mut refreshed = sample("b", "Work", None);
        refreshed.payload = "{}".into();
        db.save_profile(&refreshed)?;
        assert_eq!(db.get_profile("b")?.map(|p| p.payload), Some("{}".into()));
        refreshed.name = "Personal".into();
        db.save_profile(&refreshed)?;
        Ok(())
    }

    #[test]
    fn test_profile_crud_roundtrip() -> Result<(), AppError> {
        let db = Database::memory()?;

        db.save_profile(&sample("a", "Dev", Some(2)))?;
        db.save_profile(&sample("b", "Draw", Some(1)))?;
        db.save_profile(&sample("c", "Misc", None))?;

        // sort_order 优先，NULL 排最后
        let all = db.get_all_profiles()?;
        assert_eq!(
            all.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "a", "c"]
        );

        let got = db.get_profile("a")?.expect("profile a exists");
        assert_eq!(got.name, "Dev");
        assert!(got.payload.contains("providers"));

        // 整行替换更新
        let mut updated = sample("a", "Dev Renamed", Some(2));
        updated.updated_at = Some(2_000);
        db.save_profile(&updated)?;
        let got = db.get_profile("a")?.expect("profile a exists");
        assert_eq!(got.name, "Dev Renamed");
        assert_eq!(got.updated_at, Some(2_000));

        assert!(db.delete_profile("a")?);
        assert!(!db.delete_profile("a")?);
        assert!(db.get_profile("a")?.is_none());
        Ok(())
    }

    #[test]
    fn test_current_profile_id_is_scoped() -> Result<(), AppError> {
        let db = Database::memory()?;

        assert_eq!(db.get_current_profile_id("claude")?, None);
        assert_eq!(db.get_current_profile_id("codex")?, None);

        // 两个分组的标记互不影响
        db.set_current_profile_id("claude", Some("a"))?;
        db.set_current_profile_id("codex", Some("b"))?;
        assert_eq!(db.get_current_profile_id("claude")?, Some("a".to_string()));
        assert_eq!(db.get_current_profile_id("codex")?, Some("b".to_string()));

        db.set_current_profile_id("claude", None)?;
        assert_eq!(db.get_current_profile_id("claude")?, None);
        assert_eq!(db.get_current_profile_id("codex")?, Some("b".to_string()));

        // 重复清除应幂等
        db.set_current_profile_id("claude", None)?;
        assert_eq!(db.get_current_profile_id("claude")?, None);
        Ok(())
    }
}
