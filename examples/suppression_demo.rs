// Example demonstrating rustcop suppression directives
// Both comment-based and attribute-based styles are supported!

use std::collections::HashMap;

// Suppress all rules for the next function using a comment
// rustcop:ignore
fn badly_formatted_function() {
    let x=1+2;
    println!("Hello");
}

// Suppress specific rule with justification using a comment
// rustcop:ignore RC1001: This is legacy code, will refactor in v2.0
fn another_bad_function() {
    let y=3+4;
}

// Stack multiple suppressions with different justifications
// rustcop:ignore RC1001: Performance-critical section, formatting sacrificed for speed
// rustcop:ignore RC1002: API compatibility with legacy system
fn complex_suppressions() {
    let z=5+6;
}

// Use the attribute macro (requires rustcop in Cargo.toml)
#[rustcop::ignore]
fn ignored_with_attribute() {
    let a=7+8;
}

// Suppress specific rule with justification using attribute
#[rustcop::ignore(RC1001, justification = "Waiting on upstream dependency fix")]
pub fn partially_suppressed_attribute() {
    let b=9+10;
}

// Stack multiple attributes with different justifications per rule
#[rustcop::ignore(RC1001, justification = "Performance optimization")]
#[rustcop::ignore(RC1002, justification = "Required for API compatibility")]
pub fn multiple_stacked_attributes() {
    let c=11+12;
}

// You can also suppress at module level with an inner attribute
mod my_module {
    #![rustcop::ignore]
    
    fn foo() { let x=1; }
    fn bar() { let y=2; }
}

fn main() {
    println!("This example demonstrates rustcop suppression directives");
    badly_formatted_function();
    another_bad_function();
    complex_suppressions();
    ignored_with_attribute();
    partially_suppressed_attribute();
    multiple_stacked_attributes();
}
