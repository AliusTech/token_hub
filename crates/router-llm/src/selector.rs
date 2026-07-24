//! 路由选择器：按 level + strategy 从候选 provider 中挑选调用顺序。
//!
//! - sequential：按 level 升序排列（level 1 优先），同 level 按 weight 降序。
//! - random：同 level 内按 weight 加权随机，再按 level 升序分组。

use domain::RoutingStrategy;
use rand::Rng;

/// 一个路由候选（对应一条 model_providers 映射）。
#[derive(Debug, Clone)]
pub struct RouteCandidate {
    pub mapping_id: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub level: i32,
    pub weight: i32,
    pub strategy: RoutingStrategy,
}

/// 选择结果：一个有序的候选列表（调用时逐个尝试，失败降级）。
#[derive(Debug, Clone)]
pub struct RouteDecision {
    /// 按优先级排序的候选列表
    pub ordered: Vec<RouteCandidate>,
}

/// 根据候选集 + strategy 生成调用顺序。
pub struct RouteSelector;

impl RouteSelector {
    /// 生成有序候选列表。空候选返回空 Decision。
    pub fn select(candidates: &[RouteCandidate]) -> RouteDecision {
        if candidates.is_empty() {
            return RouteDecision { ordered: vec![] };
        }

        // 如果存在混合 strategy，以第一个候选的 strategy 为准（实际配置应统一）
        let strategy = candidates[0].strategy;

        let ordered = match strategy {
            RoutingStrategy::Sequential => Self::sequential_order(candidates),
            RoutingStrategy::Random => Self::random_order(candidates),
        };

        RouteDecision { ordered }
    }

    /// sequential：level 升序，同 level 按 weight 降序。
    fn sequential_order(candidates: &[RouteCandidate]) -> Vec<RouteCandidate> {
        let mut sorted = candidates.to_vec();
        sorted.sort_by(|a, b| a.level.cmp(&b.level).then_with(|| b.weight.cmp(&a.weight)));
        sorted
    }

    /// random：按 level 分组，组内 weight 加权随机，组间 level 升序。
    fn random_order(candidates: &[RouteCandidate]) -> Vec<RouteCandidate> {
        let mut sorted = candidates.to_vec();
        sorted.sort_by_key(|c| c.level);

        let mut result = Vec::with_capacity(sorted.len());
        let mut idx = 0;
        let mut rng = rand::thread_rng();

        while idx < sorted.len() {
            // 找出同 level 的组
            let level = sorted[idx].level;
            let mut group = Vec::new();
            while idx < sorted.len() && sorted[idx].level == level {
                group.push(sorted[idx].clone());
                idx += 1;
            }
            // 组内加权随机洗牌
            Self::weighted_shuffle(&mut group, &mut rng);
            result.extend(group);
        }
        result
    }

    /// 加权随机洗牌（按 weight 轮盘赌依次选出）。
    fn weighted_shuffle<R: Rng>(items: &mut Vec<RouteCandidate>, rng: &mut R) {
        let mut result = Vec::with_capacity(items.len());
        while !items.is_empty() {
            let total: i64 = items.iter().map(|c| c.weight.max(1) as i64).sum();
            let mut pick = rng.gen_range(0..total.max(1));
            let mut chosen_idx = 0;
            for (i, c) in items.iter().enumerate() {
                pick -= c.weight.max(1) as i64;
                if pick < 0 {
                    chosen_idx = i;
                    break;
                }
            }
            result.push(items.remove(chosen_idx));
        }
        *items = result;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, level: i32, weight: i32, strategy: RoutingStrategy) -> RouteCandidate {
        RouteCandidate {
            mapping_id: id.to_string(),
            provider_id: format!("prov_{id}"),
            upstream_model: "gpt-4o".to_string(),
            level,
            weight,
            strategy,
        }
    }

    #[test]
    fn sequential_orders_by_level_then_weight() {
        let candidates = vec![
            candidate("a", 3, 100, RoutingStrategy::Sequential),
            candidate("b", 1, 50, RoutingStrategy::Sequential),
            candidate("c", 1, 100, RoutingStrategy::Sequential),
            candidate("d", 2, 80, RoutingStrategy::Sequential),
        ];
        let decision = RouteSelector::select(&candidates);
        // level 1 优先，同 level weight 大的优先
        assert_eq!(decision.ordered[0].mapping_id, "c"); // level1 weight100
        assert_eq!(decision.ordered[1].mapping_id, "b"); // level1 weight50
        assert_eq!(decision.ordered[2].mapping_id, "d"); // level2
        assert_eq!(decision.ordered[3].mapping_id, "a"); // level3
    }

    #[test]
    fn random_groups_by_level() {
        let candidates = vec![
            candidate("a", 1, 100, RoutingStrategy::Random),
            candidate("b", 1, 100, RoutingStrategy::Random),
            candidate("c", 2, 100, RoutingStrategy::Random),
        ];
        let decision = RouteSelector::select(&candidates);
        // level 1 的两个一定在 level 2 之前
        assert_eq!(decision.ordered[0].level, 1);
        assert_eq!(decision.ordered[1].level, 1);
        assert_eq!(decision.ordered[2].level, 2);
    }

    #[test]
    fn empty_candidates_returns_empty() {
        let decision = RouteSelector::select(&[]);
        assert!(decision.ordered.is_empty());
    }

    #[test]
    fn single_candidate() {
        let candidates = vec![candidate("only", 2, 50, RoutingStrategy::Sequential)];
        let decision = RouteSelector::select(&candidates);
        assert_eq!(decision.ordered.len(), 1);
        assert_eq!(decision.ordered[0].mapping_id, "only");
    }

    #[test]
    fn random_weighted_distribution_roughly_correct() {
        // weight 90 vs 10，大量采样应约 90% 选前者在前
        let mut first_count = 0;
        let trials = 1000;
        for _ in 0..trials {
            let candidates = vec![
                candidate("heavy", 1, 90, RoutingStrategy::Random),
                candidate("light", 1, 10, RoutingStrategy::Random),
            ];
            let decision = RouteSelector::select(&candidates);
            if decision.ordered[0].mapping_id == "heavy" {
                first_count += 1;
            }
        }
        // 允许 ±10% 偏差
        let ratio = first_count as f64 / trials as f64;
        assert!(
            ratio > 0.80 && ratio < 0.99,
            "expected ~90%, got {ratio:.2}"
        );
    }
}
