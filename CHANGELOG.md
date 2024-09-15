# Changelog

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
