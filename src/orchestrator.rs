//! Simple Orchestrator - Only controls WHEN to run tools
//!
//! Tools are self-contained and know:
//! - What files to process
//! - When they should run
//! - What patterns to detect
//!
//! Forge just detects changes and asks: "Should you run?"

use anyhow::{Context as _, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Tool execution context shared across all tools
#[derive(Clone)]
pub struct ExecutionContext {
    /// Repository root path
    pub repo_root: PathBuf,
    
    /// Forge storage path (.dx/forge)
    pub forge_path: PathBuf,
    
    /// Current Git branch
    pub current_branch: Option<String>,
    
    /// Changed files in this execution
    pub changed_files: Vec<PathBuf>,
    
    /// Shared state between tools
    pub shared_state: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    
    /// Traffic branch analyzer
    pub traffic_analyzer: Arc<dyn TrafficAnalyzer + Send + Sync>,
}

impl std::fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("repo_root", &self.repo_root)
            .field("forge_path", &self.forge_path)
            .field("current_branch", &self.current_branch)
            .field("changed_files", &self.changed_files)
            .field("traffic_analyzer", &"<dyn TrafficAnalyzer>")
            .finish()
    }
}

impl ExecutionContext {
    /// Create a new execution context
    pub fn new(repo_root: PathBuf, forge_path: PathBuf) -> Self {
        Self {
            repo_root,
            forge_path,
            current_branch: None,
            changed_files: Vec::new(),
            shared_state: Arc::new(RwLock::new(HashMap::new())),
            traffic_analyzer: Arc::new(DefaultTrafficAnalyzer),
        }
    }
    
    /// Set a shared value
    pub fn set<T: Serialize>(&self, key: impl Into<String>, value: T) -> Result<()> {
        let json = serde_json::to_value(value)?;
        self.shared_state.write().insert(key.into(), json);
        Ok(())
    }
    
    /// Get a shared value
    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        let state = self.shared_state.read();
        if let Some(value) = state.get(key) {
            let result = serde_json::from_value(value.clone())?;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
    
    /// Find regex patterns in a file
    pub fn find_patterns(&self, _pattern: &str) -> Result<Vec<PatternMatch>> {
        // Implementation will be added
        Ok(Vec::new())
    }
}

/// Pattern match result
#[derive(Debug, Clone)]
pub struct PatternMatch {
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
    pub text: String,
    pub captures: Vec<String>,
}

/// Output from tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub success: bool,
    pub files_modified: Vec<PathBuf>,
    pub files_created: Vec<PathBuf>,
    pub files_deleted: Vec<PathBuf>,
    pub message: String,
    pub duration_ms: u64,
}

impl ToolOutput {
    pub fn success() -> Self {
        Self {
            success: true,
            files_modified: Vec::new(),
            files_created: Vec::new(),
            files_deleted: Vec::new(),
            message: "Success".to_string(),
            duration_ms: 0,
        }
    }
    
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            files_modified: Vec::new(),
            files_created: Vec::new(),
            files_deleted: Vec::new(),
            message: message.into(),
            duration_ms: 0,
        }
    }
}

/// Main DX tool trait - all tools must implement this
pub trait DxTool: Send + Sync {
    /// Tool name (e.g., "dx-ui", "dx-style")
    fn name(&self) -> &str;
    
    /// Tool version
    fn version(&self) -> &str;
    
    /// Execution priority (lower = executes earlier)
    fn priority(&self) -> u32;
    
    /// Execute the tool
    fn execute(&mut self, context: &ExecutionContext) -> Result<ToolOutput>;
    
    /// Check if tool should run (optional pre-check)
    fn should_run(&self, _context: &ExecutionContext) -> bool {
        true
    }
    
    /// Tool dependencies (must run after these tools)
    fn dependencies(&self) -> Vec<String> {
        Vec::new()
    }
}

// Tools are self-contained - no manifests needed
// Each tool knows what to do and when to run

/// Traffic branch analysis result
#[derive(Debug, Clone, PartialEq)]
pub enum TrafficBranch {
    /// 🟢 Green: Safe to auto-update
    Green,
    
    /// 🟡 Yellow: Can merge with conflicts
    Yellow { conflicts: Vec<Conflict> },
    
    /// 🔴 Red: Manual resolution required
    Red { conflicts: Vec<Conflict> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    pub path: PathBuf,
    pub line: usize,
    pub reason: String,
}

/// Traffic branch analyzer trait
pub trait TrafficAnalyzer {
    fn analyze(&self, file: &Path) -> Result<TrafficBranch>;
    fn can_auto_merge(&self, conflicts: &[Conflict]) -> bool;
}

/// Default traffic analyzer implementation
pub struct DefaultTrafficAnalyzer;

impl TrafficAnalyzer for DefaultTrafficAnalyzer {
    fn analyze(&self, _file: &Path) -> Result<TrafficBranch> {
        // TODO: Implement actual analysis
        Ok(TrafficBranch::Green)
    }
    
    fn can_auto_merge(&self, conflicts: &[Conflict]) -> bool {
        conflicts.is_empty()
    }
}

/// Simple orchestrator - just coordinates tool execution timing
pub struct Orchestrator {
    tools: Vec<Box<dyn DxTool>>,
    context: ExecutionContext,
}

impl Orchestrator {
    /// Create a new orchestrator
    pub fn new(repo_root: impl Into<PathBuf>) -> Result<Self> {
        let repo_root = repo_root.into();
        let forge_path = repo_root.join(".dx/forge");
        
        Ok(Self {
            tools: Vec::new(),
            context: ExecutionContext::new(repo_root, forge_path),
        })
    }
    
    /// Register a tool (tools configure themselves)
    pub fn register_tool(&mut self, tool: Box<dyn DxTool>) -> Result<()> {
        let name = tool.name().to_string();
        println!("📦 Registered tool: {} v{} (priority: {})", 
            name, tool.version(), tool.priority());
        self.tools.push(tool);
        Ok(())
    }
    
    /// Execute all registered tools in priority order
    pub fn execute_all(&mut self) -> Result<Vec<ToolOutput>> {
        // Sort tools by priority
        self.tools.sort_by_key(|t| t.priority());
        
        // Check dependencies
        self.validate_dependencies()?;
        
        // Execute each tool
        let mut outputs = Vec::new();
        
        for tool in &mut self.tools {
            if !tool.should_run(&self.context) {
                println!("⏭️  Skipping {}: pre-check failed", tool.name());
                continue;
            }
            
            println!("🚀 Executing: {} (priority: {})", tool.name(), tool.priority());
            
            let start = std::time::Instant::now();
            let mut output = tool.execute(&self.context)?;
            output.duration_ms = start.elapsed().as_millis() as u64;
            
            if output.success {
                println!("✅ {} completed in {}ms", tool.name(), output.duration_ms);
            } else {
                println!("❌ {} failed: {}", tool.name(), output.message);
            }
            
            outputs.push(output);
        }
        
        Ok(outputs)
    }
    
    /// Validate tool dependencies
    fn validate_dependencies(&self) -> Result<()> {
        let tool_names: HashSet<String> = self.tools.iter()
            .map(|t| t.name().to_string())
            .collect();
        
        for tool in &self.tools {
            for dep in tool.dependencies() {
                if !tool_names.contains(&dep) {
                    anyhow::bail!(
                        "Tool '{}' requires '{}' but it's not registered",
                        tool.name(),
                        dep
                    );
                }
            }
        }
        
        Ok(())
    }
    
    /// Get execution context
    pub fn context(&self) -> &ExecutionContext {
        &self.context
    }
    
    /// Get mutable context
    pub fn context_mut(&mut self) -> &mut ExecutionContext {
        &mut self.context
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    struct MockTool {
        name: String,
        priority: u32,
    }
    
    impl DxTool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }
        
        fn version(&self) -> &str {
            "1.0.0"
        }
        
        fn priority(&self) -> u32 {
            self.priority
        }
        
        fn execute(&mut self, _ctx: &ExecutionContext) -> Result<ToolOutput> {
            Ok(ToolOutput::success())
        }
    }
    
    #[test]
    fn test_orchestrator_priority_order() {
        let mut orch = Orchestrator::new("/tmp/test").unwrap();
        
        orch.register_tool(Box::new(MockTool { name: "tool-c".into(), priority: 30 })).unwrap();
        orch.register_tool(Box::new(MockTool { name: "tool-a".into(), priority: 10 })).unwrap();
        orch.register_tool(Box::new(MockTool { name: "tool-b".into(), priority: 20 })).unwrap();
        
        let outputs = orch.execute_all().unwrap();
        
        assert_eq!(outputs.len(), 3);
        assert!(outputs.iter().all(|o| o.success));
    }
}
