# llm-stream: Streamline Your LLM Interactions 🚀

`llm-stream` is a command-line interface (CLI) tool designed to simplify and enhance your interactions with large language models (LLMs).
It provides a seamless way to send prompts, stream responses, manage conversations, and leverage the power of LLMs directly from your terminal.

## Key Features ✨

- **Streaming Output:** Get real-time responses from LLMs as they are generated, providing a more engaging and interactive experience.
- **Conversation Management:** Easily maintain context across multiple interactions with support for different conversation history management strategies.
- **Preset Configurations:** Define and store your preferred LLM settings, API keys, and other parameters for quick and convenient access.
- **Template Support:** Utilize templates to structure your prompts and responses, ensuring consistency and efficiency.
- **Customizable Output:** Tailor the output format to your liking, whether it's plain text, JSON, or colored text for improved readability.
- **Cross-Platform Compatibility:** Works seamlessly on Linux, macOS, and Windows operating systems.

## Installation 💻

You can install `llm-stream` using the following cargo command:

```bash
cargo install llm-stream
```

## Usage 🚀

To start using `llm-stream`, simply type `llm-stream` followed by your prompt:

```bash
llm-stream 'What is the capital of France?'
```

This will send your prompt to the configured LLM and display the streamed response in your terminal.

You can also send your prompt from `stdin`.

```bash
echo -n "What is the capital of France?" | llm-stream
```

Or both.

```bash
echo -n "What is the capital of" | llm-stream - "France?"
```

> Notice the first `-`, this tells `llm-stream` that it should take the input from `stdin`, else it will only take the prompt.

### Configuration ⚙️

`llm-stream` uses a TOML configuration file to manage settings, API keys, and other customizations. The default configuration file is located at `~/.config/llm-stream.toml`.

### Presets

Presets allow you to define and store different LLM configurations, such as API keys, model endpoints, and other parameters. Here's an example of how to configure a preset for the OpenAI API:

```toml
[[preset]]
name = "openai"
api_key = "YOUR_OPENAI_API_KEY"
model = "gpt-3.5-turbo"
temperature = 0.7
max_tokens = 2048
```

Once defined, you can switch between presets using the `--preset` flag:

```bash
llm-stream --preset openai 'What is the meaning of life?'
```

### Templates

Templates provide a convenient way to structure your prompts and responses. They use the [Tera](https://keats.github.io/tera/docs/) templating language for dynamic content.

Here's an example of a template that summarizes a given text:

```toml
[[templates]]
name = "summarizer"
prompt = """
Please summarize the following text:
{{ stdin }}

{% if prompt -%}And follow this instructions:
{{ prompt }}{%- endif %}

Summary:
"""
```

You can use a template with the `--template` flag and pass the required variables:

```bash
llm-stream --template summarize text="This is a long text that needs to be summarized."
```

Templates can be run to expand the user `prompt` and also the `system` prompt. Use the `template` and `system` prompt accordingly to configure each.

You can also pass any arbitrary variable to use inside your template using the `--vars` option or through the `default_vars` option of the `Template` configuration as JSON.

For example, this template:

```toml
[[templates]]
name = "summarizer"
system = "Write your answer using {{ programming_language }}. Return only code, without any additional comments."
prompt = "{{ prompt }}"
default_vars = { programming_language = "rust" }
```

If we run it without any value it will write the output in `rust`. If we pass `--vars '{ "programming_language": "go" }'` it will return the answer using `go`.

```bash
llm-stream --vars '{ "programming_language": "javascript" }' \
  'Give me a recursive function that calculates the nth fibonacci number using dynamic programming and mnemoization'
```

> Inside the templates this variables are also available: `stdin`, `prompt`, `suffix`, and `language`.

## Contributing 🤝

We welcome contributions from the community! If you have any ideas, bug reports, or feature requests, please open an issue or submit a pull request on the [GitHub repository](https://github.com/cloudbridgeuy/llm-stream).

Let's build an awesome CLI tool for interacting with LLMs together! 🎉
