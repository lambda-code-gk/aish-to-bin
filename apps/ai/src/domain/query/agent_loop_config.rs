//! 責務: エージェントループの設定定数と max_queries の決定のみ。I/O を知らない。

use super::QueryRetry;

pub const DEFAULT_MAX_TURNS: usize = 16;
pub const DEFAULT_MAX_QUERIES_ACT: usize = 2;
pub const DEFAULT_MAX_QUERIES_PLAN: usize = 1;

/// QueryRetry モードに基づき max_queries のデフォルトを返す純関数。
pub fn default_max_queries(retry: QueryRetry) -> usize {
    match retry {
        QueryRetry::Plan => DEFAULT_MAX_QUERIES_PLAN,
        QueryRetry::Act | QueryRetry::Auto => DEFAULT_MAX_QUERIES_ACT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_has_one_query() {
        assert_eq!(default_max_queries(QueryRetry::Plan), 1);
    }

    #[test]
    fn act_and_auto_have_two_queries() {
        assert_eq!(default_max_queries(QueryRetry::Act), 2);
        assert_eq!(default_max_queries(QueryRetry::Auto), 2);
    }
}
