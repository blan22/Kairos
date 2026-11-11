use std::sync::Arc;

use kairos_core::model::{CollectContext, DirectSource, QuerySpec, Source, SourceMode, SourceName, SourceResult};
use kairos_core::ports::Clock;
use kairos_core::process::ProcessRunner;

use crate::collection::Collection;
use crate::config::GithubConfig;
use crate::defs::DEFS;
use crate::node_parser::NodeParser;

pub struct GithubSource {
    runner: Arc<dyn ProcessRunner>,
    clock: Arc<dyn Clock>,
    config: GithubConfig,
    queries: Vec<QuerySpec>,
    parser: NodeParser<'static>,
}

impl GithubSource {
    pub fn new(runner: Arc<dyn ProcessRunner>, clock: Arc<dyn Clock>, config: GithubConfig) -> Self {
        Self { runner, clock, config, queries: DEFS.iter().map(|def| def.spec()).collect(), parser: NodeParser::new() }
    }

    #[cfg(test)]
    pub(crate) fn with_parser(self, parser: NodeParser<'static>) -> Self {
        Self { parser, ..self }
    }
}

impl Source for GithubSource {
    fn name(&self) -> SourceName {
        SourceName::Github
    }

    fn queries(&self) -> &[QuerySpec] {
        &self.queries
    }

    fn mode(&self) -> SourceMode<'_> {
        SourceMode::Direct(self)
    }
}

impl DirectSource for GithubSource {
    fn collect(&self, context: &CollectContext) -> Vec<SourceResult> {
        Collection {
            runner: &self.runner,
            clock: self.clock.as_ref(),
            config: &self.config,
            parser: &self.parser,
            context,
        }
        .run()
    }
}
