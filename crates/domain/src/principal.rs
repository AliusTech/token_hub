//! 统一身份主体：三类身份（Admin / Service / API用户）归一到 Principal。

use serde::{Deserialize, Serialize};

/// 主体类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrincipalKind {
    /// 管理员（TOTP 登录，有状态 token）
    Admin,
    /// 服务系统（OAuth2 client_credentials，JWT）
    Service,
    /// 应用用户（静态 API Token）
    ApiUser,
}

/// 权限范围。Admin/Service 走 scope 鉴权；ApiUser 的 scope 通常为空（由账号策略控制模型访问）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    AdminRead,
    AdminWrite,
    AccountsRead,
    AccountsWrite,
    TokensRead,
    TokensWrite,
    CreditsRead,
    CreditsWrite,
    CreditsAdmin,
    ModelsRead,
    ModelsWrite,
    ProvidersRead,
    ProvidersWrite,
    ServicesRead,
    ServicesWrite,
    PoliciesRead,
    PoliciesWrite,
    ReportsRead,
    AuditRead,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        use Scope::*;
        match self {
            AdminRead => "admin.read",
            AdminWrite => "admin.write",
            AccountsRead => "accounts.read",
            AccountsWrite => "accounts.write",
            TokensRead => "tokens.read",
            TokensWrite => "tokens.write",
            CreditsRead => "credits.read",
            CreditsWrite => "credits.write",
            CreditsAdmin => "credits.admin",
            ModelsRead => "models.read",
            ModelsWrite => "models.write",
            ProvidersRead => "providers.read",
            ProvidersWrite => "providers.write",
            ServicesRead => "services.read",
            ServicesWrite => "services.write",
            PoliciesRead => "policies.read",
            PoliciesWrite => "policies.write",
            ReportsRead => "reports.read",
            AuditRead => "audit.read",
        }
    }

    /// 从字符串解析 scope，未知值返回 None。
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        use Scope::*;
        Some(match s {
            "admin.read" => AdminRead,
            "admin.write" => AdminWrite,
            "accounts.read" => AccountsRead,
            "accounts.write" => AccountsWrite,
            "tokens.read" => TokensRead,
            "tokens.write" => TokensWrite,
            "credits.read" => CreditsRead,
            "credits.write" => CreditsWrite,
            "credits.admin" => CreditsAdmin,
            "models.read" => ModelsRead,
            "models.write" => ModelsWrite,
            "providers.read" => ProvidersRead,
            "providers.write" => ProvidersWrite,
            "services.read" => ServicesRead,
            "services.write" => ServicesWrite,
            "policies.read" => PoliciesRead,
            "policies.write" => PoliciesWrite,
            "reports.read" => ReportsRead,
            "audit.read" => AuditRead,
            _ => return None,
        })
    }
}

/// 统一鉴权主体。所有请求经中间件解析后落入 Principal，业务层据此 + scope 决策。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub kind: PrincipalKind,
    /// Admin id / Service client_id / ApiUser account_id
    pub id: String,
    /// ApiUser 专属：关联的账号 ID（Admin/Service 为 None）
    pub account_id: Option<String>,
    /// Admin/Service 的 scope 集合
    pub scopes: Vec<Scope>,
}

impl Principal {
    pub fn has_scope(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope)
    }

    /// 系统管理员（admin.write 的超级权限）
    pub fn is_super_admin(&self) -> bool {
        self.kind == PrincipalKind::Admin && self.has_scope(Scope::AdminWrite)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_roundtrip() {
        for s in [
            Scope::AdminRead,
            Scope::CreditsAdmin,
            Scope::AuditRead,
            Scope::ProvidersWrite,
        ] {
            assert_eq!(Scope::from_str_lossy(s.as_str()), Some(s));
        }
        assert!(Scope::from_str_lossy("unknown").is_none());
    }
}
