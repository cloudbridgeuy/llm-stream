# Changelog

## [Unreleased]

### Fixed

- **llm-stream**: A conversation's `title`, `description`, and `parent` are no
  longer destroyed the first time it is continued. `merge_args_and_cache` did not
  restore them from the cache file, so the rewrite that follows the response
  dropped the keys. An explicit `--title` or `--description` still wins, and
  remains the way to rename a conversation.

## [0.5.1] - 2025-07-08

### Changed

#### llm_stream

- **refactor**: Embed theme data instead of loading from file path

## [0.5.0] - 2025-07-08

### Added

- **llm_stream**: DeepSeek API integration with reasoning support
- **llm_stream**: Support for editing prompts in external editor
- **llm_stream**: Support for displaying reasoning content from OpenAI responses
- **llm_stream**: Centralized syntax highlighting with theme support
- **llm_stream**: Nom-based markdown parser with text wrapping and improved highlighting
- **llm_stream**: Tokyonight theme support in build script
- **jina**: CLI tool for HTML to Markdown conversion using Jina AI API

### Changed

- **llm_stream**: Refactored syntax highlighting to use centralized printer module
- **llm_stream**: Removed direct syntect dependencies from prelude
- **llm_stream**: Simplified text accumulation logic in stream handlers

### Fixed

- **llm_stream**: Prevent sending empty messages in conversation

### Removed

- **llm_stream**: Language and theme configuration options
- **build**: Bat dependency and related code cleanup
- **llm_stream**: Syntax highlighting functionality and associated build script logic for theme asset management

## [llm-stream/0.4.0] - 2024-10-04

### Added

- Support for Groq API in llm_stream
- Preset and template listing functionality in llm_stream
- Support for Ollama API in llm_stream

### Changed

- Improved Api enum and code organization in llm_stream

### Other

- Added option to skip changelog generation during publish in xtask

## [0.4.0] - 2024-10-02

### Added

#### LLM Stream

- Support for Ollama API ([411b783](https://github.com/yourusername/yourrepository/commit/411b783), [544d621](https://github.com/yourusername/yourrepository/commit/544d621))
- Description and title options for conversations ([eaa13e7](https://github.com/yourusername/yourrepository/commit/eaa13e7))
- Conversation display and color control options ([213f1df](https://github.com/yourusername/yourrepository/commit/213f1df))
- Conversation forking functionality ([cd549d0](https://github.com/yourusername/yourrepository/commit/cd549d0))
- Option to continue from last conversation ([02fa9a8](https://github.com/yourusername/yourrepository/commit/02fa9a8))
- Conversation caching and continuation ([80990f2](https://github.com/yourusername/yourrepository/commit/80990f2))

#### Config

- Support for loading templates from directory ([515b083](https://github.com/yourusername/yourrepository/commit/515b083))

### Changed

#### LLM Stream

- Changed println! to eprintln! for non-content output ([02fa9a8](https://github.com/yourusername/yourrepository/commit/02fa9a8))

## llm-stream [0.3.0] - 2024-09-21

### Added

- Add description and title options for conversations
- Add conversation display and color control options
- Add conversation forking functionality
- Add option to continue from last conversation
- Add conversation caching and continuation
- Add support for loading templates from directory

### Changed

- Change println! to eprintln! for non-content output

### Exposed

- Expose EventsourceError from eventsource_client

### Chore

- Bump version to 0.3.1

## 0.3.1

### Added

- feat(llm_stream): add conversation printing and dry run options
- feat(llm_stream): enhance system message handling in conversations
- feat(llm_stream): add CLI options to display config file and directory
- feat(llm_stream): add 'see' submodule for extended functionality

### Changed

- feat(llm_stream): bumped library to v0.3.1
- chore(llm_stream): expose EventsourceError from eventsource_client
- chore: bumped llm-stream version to 0.2.0
- refactor(llm_stream): update template and config handling
- refactor(llm-stream): improve merge_args_and_config function logic
- refactor(llm-stream): add new fields and reorder existing ones in Args, Preset, and Config structs
- refactor(llm-stream): restructure argument handling and config management
- refactor(llm-stream): reorganize argument parsing logic
- refactor(llm-stream): flatten Args struct by removing nested Globals
- refactor(llm-stream): centralize argument processing and standardize API interactions
- refactor(llm_stream): improve input handling and conversation management

### Removed

- build: remove 'see' submodule from lib directory

### Testing

- test(llm_stream): adjust tests for new conversation handling
- test(llm_stream): add new tests for system and template handling
- test(llm-stream): add unit tests for merge_args_and_config function

### Miscellaneous

- chore: add rust-analyzer configuration file
- chore(llm-stream): bump version to 0.2.0-WIP

## 0.3.0

### Changed

- Streamline client implementations

## 0.2.0

### Changed

- **llm_stream**: Update llm_stream dependency to version 0.2.0

### Chore

- **llm_stream**: Bump version to 0.2.0

## 0.0.3

### Added

- feat(xtask): new changelog command added

### Changed

- refactor: remove unused imports and variables
- refactor: move common code to shared module

### Fixed

- fix: correct typos in README and documentation

### Global Changes

- chore: update dependencies to latest versions
- docs: improve project documentation and examples
- test: enhance test coverage for core functionality

[0.0.3]: https://github.com/username/repo/compare/0.0.2...0.0.3

## 0.0.1

### Added

#### Global

- Initial project setup and configuration
