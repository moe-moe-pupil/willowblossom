use rand::RngExt;

const DEFAULT_DIE_FACES: u32 = 100;
const MAX_EXPRESSION_BYTES: usize = 256;
const MAX_DICE_PER_TERM: u32 = 100;
const MAX_TOTAL_DICE: u32 = 1_000;
const MAX_DIE_FACES: u32 = 1_000_000;
const MAX_LITERAL: u64 = 1_000_000_000;
const MAX_PARSE_DEPTH: usize = 16;
const MAX_ABSOLUTE_RESULT: f64 = 1_000_000_000_000_000.0;

#[derive(Debug, Clone, PartialEq)]
enum DiceExpression {
    Number(f64),
    Dice {
        count: u32,
        faces: u32,
    },
    Negate(Box<DiceExpression>),
    Binary {
        left: Box<DiceExpression>,
        operator: BinaryOperator,
        right: Box<DiceExpression>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, PartialEq)]
struct ParsedDiceCommand {
    formula: String,
    description: Option<String>,
    expression: DiceExpression,
}

#[derive(Debug, PartialEq, Eq)]
struct DiceTermResult {
    notation: String,
    values: Vec<u32>,
}

pub(super) fn roll_dice_command(text: &str) -> Option<String> {
    let parsed = match parse_dice_command(text)? {
        Ok(parsed) => parsed,
        Err(error) => return Some(format!("骰点表达式无效：{error}")),
    };
    let mut rng = rand::rng();
    let mut dice_terms = Vec::new();
    let result = match evaluate_expression(
        &parsed.expression,
        &mut dice_terms,
        &mut |faces| rng.random_range(1..=faces),
    ) {
        Ok(result) => result,
        Err(error) => return Some(format!("骰点失败：{error}")),
    };

    let mut lines = vec![format!("掷骰【{}】", parsed.formula)];
    if !dice_terms.is_empty() {
        lines.push(
            dice_terms
                .iter()
                .map(|term| format!("{}={:?}", term.notation, term.values))
                .collect::<Vec<_>>()
                .join("；"),
        );
    }
    lines.push(format!(
        "结果：{}",
        format_result(result)
    ));
    if let Some(description) = parsed.description {
        lines.push(format!("说明：{description}"));
    }
    Some(lines.join("\n"))
}

fn parse_dice_command(text: &str) -> Option<Result<ParsedDiceCommand, String>> {
    let body = text
        .trim()
        .strip_prefix('.')
        .or_else(|| text.trim().strip_prefix('。'))?
        .trim_start();
    let formula_and_description = dice_command_formula(body)?;
    Some(parse_formula_and_description(
        formula_and_description,
    ))
}

fn dice_command_formula(body: &str) -> Option<&str> {
    if body
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("roll"))
    {
        let remainder = &body[4..];
        if remainder.is_empty() || starts_dice_formula(remainder) {
            return Some(remainder.trim_start());
        }
    }
    if body
        .as_bytes()
        .first()
        .is_some_and(|byte| matches!(byte, b'r' | b'R'))
    {
        let remainder = &body[1..];
        if remainder.is_empty() || starts_dice_formula(remainder) {
            return Some(remainder.trim_start());
        }
        return None;
    }
    if body
        .as_bytes()
        .first()
        .is_some_and(|byte| matches!(byte, b'd' | b'D'))
    {
        let remainder = &body[1..];
        if remainder.is_empty() {
            return Some("d");
        }
        if remainder
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            let remainder = remainder.trim_start();
            return starts_dice_formula(remainder).then_some(remainder);
        }
        if remainder
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            return Some(body);
        }
        return None;
    }
    None
}

fn starts_dice_formula(text: &str) -> bool {
    text.trim_start().as_bytes().first().is_some_and(|byte| {
        byte.is_ascii_digit() || matches!(byte, b'd' | b'D' | b'(' | b'+' | b'-')
    })
}

fn parse_formula_and_description(text: &str) -> Result<ParsedDiceCommand, String> {
    let text = if text.trim().is_empty() { "d" } else { text.trim() };
    if text.len() > MAX_EXPRESSION_BYTES {
        return Err(format!(
            "表达式不能超过{MAX_EXPRESSION_BYTES}字节"
        ));
    }
    let (formula_text, hash_description) = text
        .split_once('#')
        .map(|(formula, description)| (formula, Some(description)))
        .unwrap_or((text, None));
    let mut parser = DiceParser::new(formula_text);
    let expression = parser.parse_expression(0)?;
    let parsed_end = parser.position;
    let trailing_description = formula_text[parsed_end..].trim();
    let formula = formula_text[..parsed_end].trim();
    if formula.is_empty() {
        return Err("缺少骰点表达式".to_owned());
    }
    let description = hash_description
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .or_else(|| (!trailing_description.is_empty()).then_some(trailing_description))
        .map(str::to_owned);
    Ok(ParsedDiceCommand {
        formula: formula.to_owned(),
        description,
        expression,
    })
}

struct DiceParser<'a> {
    input: &'a str,
    position: usize,
    total_dice: u32,
}

impl<'a> DiceParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            position: 0,
            total_dice: 0,
        }
    }

    fn parse_expression(&mut self, depth: usize) -> Result<DiceExpression, String> {
        let mut expression = self.parse_term(depth)?;
        loop {
            self.skip_whitespace();
            let operator = match self.peek_byte() {
                Some(b'+') => BinaryOperator::Add,
                Some(b'-') => BinaryOperator::Subtract,
                _ => break,
            };
            self.position += 1;
            let right = self.parse_term(depth)?;
            expression = DiceExpression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    fn parse_term(&mut self, depth: usize) -> Result<DiceExpression, String> {
        let mut expression = self.parse_factor(depth)?;
        loop {
            self.skip_whitespace();
            let operator = match self.peek_byte() {
                Some(b'*') => BinaryOperator::Multiply,
                Some(b'/') => BinaryOperator::Divide,
                _ => break,
            };
            self.position += 1;
            let right = self.parse_factor(depth)?;
            expression = DiceExpression::Binary {
                left: Box::new(expression),
                operator,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    fn parse_factor(&mut self, depth: usize) -> Result<DiceExpression, String> {
        if depth > MAX_PARSE_DEPTH {
            return Err(format!(
                "括号嵌套不能超过{MAX_PARSE_DEPTH}层"
            ));
        }
        self.skip_whitespace();
        match self.peek_byte() {
            Some(b'+') => {
                self.position += 1;
                self.parse_factor(depth)
            },
            Some(b'-') => {
                self.position += 1;
                Ok(DiceExpression::Negate(Box::new(
                    self.parse_factor(depth)?,
                )))
            },
            Some(b'(') => {
                self.position += 1;
                let expression = self.parse_expression(depth + 1)?;
                self.skip_whitespace();
                if self.peek_byte() != Some(b')') {
                    return Err("括号没有闭合".to_owned());
                }
                self.position += 1;
                Ok(expression)
            },
            Some(b'd' | b'D') => {
                self.position += 1;
                self.parse_dice(1)
            },
            Some(byte) if byte.is_ascii_digit() => {
                let number = self.parse_unsigned_integer()?;
                if matches!(self.peek_byte(), Some(b'd' | b'D')) {
                    self.position += 1;
                    let count = u32::try_from(number)
                        .map_err(|_| format!("每项骰子数不能超过{MAX_DICE_PER_TERM}"))?;
                    self.parse_dice(count)
                } else {
                    Ok(DiceExpression::Number(number as f64))
                }
            },
            _ => Err("应输入数字、骰子或括号".to_owned()),
        }
    }

    fn parse_dice(&mut self, count: u32) -> Result<DiceExpression, String> {
        if count == 0 || count > MAX_DICE_PER_TERM {
            return Err(format!(
                "每项骰子数必须在1到{MAX_DICE_PER_TERM}之间"
            ));
        }
        let faces = if self.peek_byte().is_some_and(|byte| byte.is_ascii_digit()) {
            u32::try_from(self.parse_unsigned_integer()?)
                .map_err(|_| format!("骰子面数不能超过{MAX_DIE_FACES}"))?
        } else {
            DEFAULT_DIE_FACES
        };
        if faces == 0 || faces > MAX_DIE_FACES {
            return Err(format!(
                "骰子面数必须在1到{MAX_DIE_FACES}之间"
            ));
        }
        self.total_dice = self
            .total_dice
            .checked_add(count)
            .filter(|total| *total <= MAX_TOTAL_DICE)
            .ok_or_else(|| format!("一次最多投掷{MAX_TOTAL_DICE}颗骰子"))?;
        Ok(DiceExpression::Dice { count, faces })
    }

    fn parse_unsigned_integer(&mut self) -> Result<u64, String> {
        let start = self.position;
        while self.peek_byte().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
        }
        let value = self.input[start..self.position]
            .parse::<u64>()
            .map_err(|_| "数字过大".to_owned())?;
        if value > MAX_LITERAL {
            return Err(format!("数字不能超过{MAX_LITERAL}"));
        }
        Ok(value)
    }

    fn skip_whitespace(&mut self) {
        while self
            .peek_byte()
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            self.position += 1;
        }
    }

    fn peek_byte(&self) -> Option<u8> { self.input.as_bytes().get(self.position).copied() }
}

fn evaluate_expression(
    expression: &DiceExpression,
    dice_terms: &mut Vec<DiceTermResult>,
    roll: &mut impl FnMut(u32) -> u32,
) -> Result<f64, String> {
    let result = match expression {
        DiceExpression::Number(number) => *number,
        DiceExpression::Dice { count, faces } => {
            let values = (0..*count).map(|_| roll(*faces)).collect::<Vec<_>>();
            let total = values.iter().map(|value| *value as f64).sum();
            dice_terms.push(DiceTermResult {
                notation: format!("{count}d{faces}"),
                values,
            });
            total
        },
        DiceExpression::Negate(expression) => -evaluate_expression(expression, dice_terms, roll)?,
        DiceExpression::Binary {
            left,
            operator,
            right,
        } => {
            let left = evaluate_expression(left, dice_terms, roll)?;
            let right = evaluate_expression(right, dice_terms, roll)?;
            match operator {
                BinaryOperator::Add => left + right,
                BinaryOperator::Subtract => left - right,
                BinaryOperator::Multiply => left * right,
                BinaryOperator::Divide if right.abs() < f64::EPSILON => {
                    return Err("不能除以零".to_owned());
                },
                BinaryOperator::Divide => left / right,
            }
        },
    };
    if !result.is_finite() || result.abs() > MAX_ABSOLUTE_RESULT {
        return Err("计算结果过大".to_owned());
    }
    Ok(result)
}

fn format_result(result: f64) -> String {
    if result.fract().abs() < f64::EPSILON {
        format!("{result:.0}")
    } else {
        let formatted = format!("{result:.4}");
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_roll_commands_accept_default_and_explicit_dice() {
        let default = parse_dice_command(".rd").unwrap().unwrap();
        assert_eq!(default.formula, "d");
        assert_eq!(
            default.expression,
            DiceExpression::Dice {
                count: 1,
                faces: 100
            }
        );

        let explicit = parse_dice_command("。r2D3").unwrap().unwrap();
        assert_eq!(
            explicit.expression,
            DiceExpression::Dice { count: 2, faces: 3 }
        );
    }

    #[test]
    fn foundry_style_aliases_arithmetic_parentheses_and_descriptions_parse() {
        for command in [".r 2d6+3", ".roll (2d6 + d4) / 2", ".d 2d6+3"] {
            assert!(
                parse_dice_command(command).unwrap().is_ok(),
                "{command}"
            );
        }
        let parsed = parse_dice_command(".roll 1d20 + 5 # 侦查")
            .unwrap()
            .unwrap();
        assert_eq!(parsed.formula, "1d20 + 5");
        assert_eq!(
            parsed.description.as_deref(),
            Some("侦查")
        );
    }

    #[test]
    fn evaluation_reports_each_die_and_obeys_precedence() {
        let parsed = parse_dice_command(".r 2d3+4*2").unwrap().unwrap();
        let mut rolls = [1, 3].into_iter();
        let mut terms = Vec::new();
        let result = evaluate_expression(
            &parsed.expression,
            &mut terms,
            &mut |_| rolls.next().unwrap(),
        )
        .unwrap();

        assert_eq!(result, 12.0);
        assert_eq!(terms[0].values, vec![1, 3]);
    }

    #[test]
    fn malformed_and_abusive_rolls_return_clear_errors() {
        for command in [".r 0d6", ".r 101d6", ".r d0", ".r (d6", ".r d6/0"] {
            let response = roll_dice_command(command).unwrap();
            assert!(
                response.starts_with("骰点表达式无效：") || response.starts_with("骰点失败："),
                "{command}: {response}"
            );
        }
    }

    #[test]
    fn unrelated_private_commands_are_not_claimed() {
        assert!(parse_dice_command(".状态").is_none());
        assert!(parse_dice_command("普通聊天").is_none());
        assert!(parse_dice_command(".random").is_none());
        assert!(parse_dice_command(".detect magic").is_none());
        assert!(parse_dice_command(".weave").is_none());
    }
}
