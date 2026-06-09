use thiserror::Error;

/// Geello 统一错误类型
///
/// 替代裸 `String` 错误，提供结构化错误信息和错误链追踪。
#[derive(Debug, Error)]
pub enum GeelloError {
    /// 渲染错误
    #[error("render error: {0}")]
    Render(String),

    /// GPU 相关错误
    #[error("gpu error: {0}")]
    Gpu(String),

    /// IO 错误
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// 序列化/反序列化错误 (TOML, JSON 等)
    #[error("serialization error: {0}")]
    Serde(String),

    /// 配置错误
    #[error("config error: {0}")]
    Config(String),

    /// 缓存错误
    #[error("cache error: {0}")]
    Cache(String),

    /// 数据解析错误 (GeoJSON, TIFF 等)
    #[error("data parse error: {0}")]
    DataParse(String),

    /// 网络请求错误
    #[error("http error: {0}")]
    Http(String),

    /// 投影/坐标变换错误
    #[error("projection error: {0}")]
    Proj(String),

    /// 参数验证错误
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// 资源未找到
    #[error("not found: {0}")]
    NotFound(String),

    /// 内部错误（通常不应发生）
    #[error("internal error: {0}")]
    Internal(String),
}

impl GeelloError {
    /// 从 vello/wgpu 错误创建
    pub fn render<E: std::fmt::Display>(e: E) -> Self {
        GeelloError::Render(e.to_string())
    }

    /// 从 wgpu 错误创建 GPU 错误
    pub fn gpu<E: std::fmt::Display>(e: E) -> Self {
        GeelloError::Gpu(e.to_string())
    }

    /// 从序列化错误创建
    pub fn serde<E: std::fmt::Display>(e: E) -> Self {
        GeelloError::Serde(e.to_string())
    }
}

/// Alias for Result with GeelloError
pub type GeelloResult<T> = Result<T, GeelloError>;
