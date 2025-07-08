use std::sync::LazyLock;

use nom::{
    bytes::complete::{tag, take_until},
    character::complete::{line_ending, not_line_ending},
    combinator::opt,
    error::Error,
    IResult,
};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use syntect::{
    easy::HighlightLines,
    highlighting::{Style, Theme, ThemeSet},
    parsing::{SyntaxReference, SyntaxSet},
    util::{as_24_bit_terminal_escaped, LinesWithEndings},
};

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let theme_data = include_str!("../assets/themes/tokyonight/tokyonight-storm.tmTheme");
    ThemeSet::load_from_reader(&mut std::io::Cursor::new(theme_data))
        .unwrap()
});
static MARKDOWN_SYNTAX: LazyLock<&SyntaxReference> =
    LazyLock::new(|| SYNTAX_SET.find_syntax_by_name("Markdown").unwrap());
static TERMINAL_WIDTH: LazyLock<usize> = LazyLock::new(|| {
    crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80)
});

pub fn markdown_to_24_bit_terminal_escaped(markdown: &str) -> String {
    let mut sr = SYNTAX_SET.find_syntax_plain_text();
    let mut output = String::new();
    let mut code = String::new();
    let mut code_block = false;

    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
                let lang = lang.trim();
                sr = SYNTAX_SET
                    .find_syntax_by_token(lang)
                    .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
                code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                let mut highlighter = HighlightLines::new(sr, &THEME);
                for line in LinesWithEndings::from(&format!("\n{}\n", code)) {
                    let ranges: Vec<(Style, &str)> =
                        highlighter.highlight_line(line, &SYNTAX_SET).unwrap();
                    let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
                    output.push_str(&escaped);
                }

                code = String::new();
                code_block = false;
            }

            Event::Text(t) => {
                if code_block {
                    code.push_str(&t);
                } else {
                    output.push_str(&t);
                }
            }

            Event::Start(Tag::Paragraph) => {
                if !output.is_empty() {
                    output.push('\n');
                }
            }

            _ => (),
        }
    }

    output
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarkdownElement {
    Text(String),
    Code { language: String, content: String },
}

fn parse_code_fence_start(input: &str) -> IResult<&str, String> {
    let (input, _) = tag("```")(input)?;
    let (input, language) = opt(not_line_ending)(input)?;
    let (input, _) = opt(line_ending)(input)?;

    let language = language
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "txt".to_string());

    Ok((input, language))
}

fn parse_code_fence_end(input: &str) -> IResult<&str, ()> {
    let (input, _) = tag("```")(input)?;
    let (input, _) = opt(line_ending)(input)?;
    Ok((input, ()))
}

fn parse_code_block(input: &str) -> IResult<&str, MarkdownElement> {
    let (input, language) = parse_code_fence_start(input)?;

    // Try to find closing fence
    if let Ok((remaining, content)) = take_until::<&str, &str, Error<&str>>("```")(input) {
        // Found closing fence, parse it
        if let Ok((final_input, _)) = parse_code_fence_end(remaining) {
            return Ok((
                final_input,
                MarkdownElement::Code {
                    language,
                    content: content.to_string(),
                },
            ));
        }
    }

    // No closing fence found, treat as unclosed code block
    Ok((
        "",
        MarkdownElement::Code {
            language,
            content: input.to_string(),
        },
    ))
}

fn parse_text_content(input: &str) -> IResult<&str, MarkdownElement> {
    let (input, content) = take_until("```")(input)?;
    let wrapped_content = wrap_text_to_terminal_width(content);
    Ok((input, MarkdownElement::Text(wrapped_content)))
}

fn parse_remaining_text(input: &str) -> IResult<&str, MarkdownElement> {
    let wrapped_content = wrap_text_to_terminal_width(input);
    Ok(("", MarkdownElement::Text(wrapped_content)))
}

fn wrap_text_to_terminal_width(text: &str) -> String {
    let width = *TERMINAL_WIDTH;
    let mut result = String::new();
    
    // Split by newlines but preserve empty lines
    let lines: Vec<&str> = text.split('\n').collect();
    
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        
        if line.len() <= width {
            // Line fits within width, keep as is
            result.push_str(line);
        } else {
            // Line needs wrapping
            let words: Vec<&str> = line.split_whitespace().collect();
            let mut current_line = String::new();
            
            for word in words {
                // Check if adding this word would exceed width
                let potential_length = if current_line.is_empty() {
                    word.len()
                } else {
                    current_line.len() + 1 + word.len() // +1 for space
                };
                
                if potential_length <= width {
                    // Word fits, add it to current line
                    if !current_line.is_empty() {
                        current_line.push(' ');
                    }
                    current_line.push_str(word);
                } else {
                    // Word doesn't fit, start new line
                    if !current_line.is_empty() {
                        result.push_str(&current_line);
                        result.push('\n');
                        current_line.clear();
                    }
                    
                    // Handle very long words that exceed width
                    if word.len() > width {
                        // Split the word itself
                        let mut remaining_word = word;
                        while !remaining_word.is_empty() {
                            let chunk_size = width.min(remaining_word.len());
                            let chunk = &remaining_word[..chunk_size];
                            
                            result.push_str(chunk);
                            remaining_word = &remaining_word[chunk_size..];
                            
                            if !remaining_word.is_empty() {
                                result.push('\n');
                            }
                        }
                    } else {
                        // Normal word, start new line with it
                        current_line.push_str(word);
                    }
                }
            }
            
            // Add remaining content in current_line
            if !current_line.is_empty() {
                result.push_str(&current_line);
            }
        }
    }
    
    result
}

pub fn parse_markdown(raw: &str) -> Vec<MarkdownElement> {
    let mut elements = Vec::new();
    let mut remaining = raw;

    while !remaining.is_empty() {
        let original_remaining = remaining;

        // Try to parse a code block first
        if let Ok((rest, code_element)) = parse_code_block(remaining) {
            elements.push(code_element);
            remaining = rest;
        }
        // If no code block, try to parse text until next code fence
        else if let Ok((rest, text_element)) = parse_text_content(remaining) {
            if !text_element.eq(&MarkdownElement::Text("".to_string())) {
                elements.push(text_element);
            }
            remaining = rest;
        }
        // If no code fence found, treat remaining as text and exit
        else {
            let (_, text_element) = parse_remaining_text(remaining).unwrap();
            if !text_element.eq(&MarkdownElement::Text("".to_string())) {
                elements.push(text_element);
            }
            break;
        }

        // Safety check to prevent infinite loops
        if remaining == original_remaining {
            // If we haven't consumed any input, force consumption by taking first char as text
            let first_char = remaining.chars().next().unwrap().to_string();
            let wrapped_first_char = wrap_text_to_terminal_width(&first_char);
            elements.push(MarkdownElement::Text(wrapped_first_char));
            remaining = &remaining[1..];
        }
    }

    elements
}

fn highlight_code_content(content: &str, language: &str) -> String {
    let syntax = SYNTAX_SET
        .find_syntax_by_token(language)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    let mut highlighter = HighlightLines::new(syntax, &THEME);
    let mut output = String::new();

    for line in LinesWithEndings::from(content) {
        let ranges: Vec<(Style, &str)> = highlighter.highlight_line(line, &SYNTAX_SET).unwrap();
        let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
        output.push_str(&escaped);
    }

    output
}

fn highlight_text_content(content: &str) -> String {
    let mut highlighter = HighlightLines::new(&MARKDOWN_SYNTAX, &THEME);
    let mut output = String::new();

    for line in LinesWithEndings::from(content) {
        let ranges: Vec<(Style, &str)> = highlighter.highlight_line(line, &SYNTAX_SET).unwrap();
        let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
        output.push_str(&escaped);
    }

    output
}

pub fn highlight_markdown(raw: &str) -> String {
    let elements = parse_markdown(raw);
    let mut output = String::new();

    for element in elements {
        match element {
            MarkdownElement::Code { language, content } => {
                output.push_str(&highlight_code_content(&content, &language));
            }
            MarkdownElement::Text(content) => {
                output.push_str(&highlight_text_content(&content));
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plain_text() {
        let input = "This is just plain text.";
        let result = parse_markdown(input);
        assert_eq!(
            result,
            vec![MarkdownElement::Text(
                "This is just plain text.".to_string()
            )]
        );
    }

    #[test]
    fn test_parse_code_block_with_language() {
        let input = "```rust\nfn main() {\n    println!(\"Hello, world!\");\n}\n```";
        let result = parse_markdown(input);
        assert_eq!(
            result,
            vec![MarkdownElement::Code {
                language: "rust".to_string(),
                content: "fn main() {\n    println!(\"Hello, world!\");\n}\n".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_code_block_without_language() {
        let input = "```\nsome code\n```";
        let result = parse_markdown(input);
        assert_eq!(
            result,
            vec![MarkdownElement::Code {
                language: "txt".to_string(),
                content: "some code\n".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_mixed_content() {
        let input = "Here's some text:\n\n```python\nprint('Hello')\n```\n\nAnd more text after.";
        let result = parse_markdown(input);
        assert_eq!(
            result,
            vec![
                MarkdownElement::Text("Here's some text:\n\n".to_string()),
                MarkdownElement::Code {
                    language: "python".to_string(),
                    content: "print('Hello')\n".to_string(),
                },
                MarkdownElement::Text("\nAnd more text after.".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_multiple_code_blocks() {
        let input = "```rust\nlet x = 5;\n```\n\nSome text\n\n```python\nprint('hi')\n```";
        let result = parse_markdown(input);
        assert_eq!(
            result,
            vec![
                MarkdownElement::Code {
                    language: "rust".to_string(),
                    content: "let x = 5;\n".to_string(),
                },
                MarkdownElement::Text("\nSome text\n\n".to_string()),
                MarkdownElement::Code {
                    language: "python".to_string(),
                    content: "print('hi')\n".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_parse_empty_input() {
        let input = "";
        let result = parse_markdown(input);
        assert_eq!(result, vec![]);
    }

    #[test]
    fn test_parse_code_block_with_whitespace_language() {
        let input = "```  javascript  \nconsole.log('test');\n```";
        let result = parse_markdown(input);
        assert_eq!(
            result,
            vec![MarkdownElement::Code {
                language: "javascript".to_string(),
                content: "console.log('test');\n".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_code_block_empty_language() {
        let input = "```   \nsome code\n```";
        let result = parse_markdown(input);
        assert_eq!(
            result,
            vec![MarkdownElement::Code {
                language: "txt".to_string(),
                content: "some code\n".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_unclosed_code_block_with_language() {
        let input = "```javascript\nfunction() {\n  console.log(\"Hello, World!\");\n}";
        let result = parse_markdown(input);
        assert_eq!(
            result,
            vec![MarkdownElement::Code {
                language: "javascript".to_string(),
                content: "function() {\n  console.log(\"Hello, World!\");\n}".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_unclosed_code_block_without_language() {
        let input = "```\nsome code\nmore code";
        let result = parse_markdown(input);
        assert_eq!(
            result,
            vec![MarkdownElement::Code {
                language: "txt".to_string(),
                content: "some code\nmore code".to_string(),
            }]
        );
    }

    #[test]
    fn test_parse_mixed_content_with_unclosed_code_block() {
        let input = "Here's some text:\n\n```python\nprint('Hello')\nprint('World')";
        let result = parse_markdown(input);
        assert_eq!(
            result,
            vec![
                MarkdownElement::Text("Here's some text:\n\n".to_string()),
                MarkdownElement::Code {
                    language: "python".to_string(),
                    content: "print('Hello')\nprint('World')".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_wrap_text_short_line() {
        let text = "This is a short line.";
        let wrapped = wrap_text_to_terminal_width(text);
        assert_eq!(wrapped, "This is a short line.");
    }

    #[test]
    fn test_wrap_text_long_line() {
        // Create a long line that will exceed typical terminal width
        let text = "This is a very long line that should definitely exceed the terminal width and therefore needs to be wrapped at appropriate word boundaries to ensure readability.";
        let wrapped = wrap_text_to_terminal_width(text);
        
        // Check that no line exceeds terminal width
        for line in wrapped.lines() {
            assert!(line.len() <= *TERMINAL_WIDTH, "Line too long: '{}'", line);
        }
        
        // Check that the text is preserved (all words should still be there)
        let original_words: Vec<&str> = text.split_whitespace().collect();
        let wrapped_words: Vec<&str> = wrapped.split_whitespace().collect();
        assert_eq!(original_words, wrapped_words);
    }

    #[test]
    fn test_wrap_text_multiple_lines() {
        let text = "Short line.\nThis is a very long line that should definitely exceed the terminal width and therefore needs to be wrapped.\nAnother short line.";
        let wrapped = wrap_text_to_terminal_width(text);
        
        // Check that no line exceeds terminal width
        for line in wrapped.lines() {
            assert!(line.len() <= *TERMINAL_WIDTH, "Line too long: '{}'", line);
        }
        
        // Check that short lines are preserved
        let lines: Vec<&str> = wrapped.lines().collect();
        assert_eq!(lines[0], "Short line.");
        assert_eq!(lines[lines.len() - 1], "Another short line.");
    }

    #[test]
    fn test_wrap_text_preserve_empty_lines() {
        let text = "First line.\n\nThird line.";
        let wrapped = wrap_text_to_terminal_width(text);
        
        let lines: Vec<&str> = wrapped.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "First line.");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "Third line.");
    }

    #[test]
    fn test_wrap_text_very_long_word() {
        // Create a single word that exceeds terminal width
        let long_word = "a".repeat(*TERMINAL_WIDTH + 10);
        let wrapped = wrap_text_to_terminal_width(&long_word);
        
        // Should be split into chunks
        for line in wrapped.lines() {
            assert!(line.len() <= *TERMINAL_WIDTH, "Line too long: '{}'", line);
        }
        
        // All characters should be preserved
        let wrapped_chars: String = wrapped.chars().filter(|&c| c != '\n').collect();
        assert_eq!(wrapped_chars, long_word);
    }

    #[test]
    fn test_parse_text_with_wrapping() {
        // Test that text parsing applies wrapping
        let long_text = "This is a very long line that should definitely exceed the terminal width and therefore needs to be wrapped at appropriate word boundaries.";
        let input = format!("{}```", long_text);
        
        if let Ok((_, element)) = parse_text_content(&input) {
            if let MarkdownElement::Text(content) = element {
                // Check that no line exceeds terminal width
                for line in content.lines() {
                    assert!(line.len() <= *TERMINAL_WIDTH, "Line too long: '{}'", line);
                }
            } else {
                panic!("Expected Text element");
            }
        } else {
            panic!("Failed to parse text content");
        }
    }
}
