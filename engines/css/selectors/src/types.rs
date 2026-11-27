//! Selector representation.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleSelector {
    Universal,
    /// A type selector, as written: case-insensitive for HTML elements, exact otherwise.
    Type(String),
    /// Elements in this namespace (`""` is no namespace), from `ns|E`, `|E` or a default
    /// namespace declared with `@namespace`.
    Namespace(String),
    Class(String),
    Id(String),
    AttrPresent(String),
    AttrEquals(String, String),
    /// `[name op value flag]`. The name is as written; the flag is `Some(true)` for `i`,
    /// `Some(false)` for `s`, and `None` for HTML's default case sensitivity.
    AttrMatch(String, String, char, Option<bool>),
    Root,
    /// `:scope`: the scoping element of a query, or the root element in a stylesheet.
    Scope,
    Hover,
    Focus,
    FocusWithin,
    Active,
    Checked,
    Disabled,
    Enabled,
    Link,
    Empty,
    FirstChild,
    LastChild,
    OnlyChild,
    NthChild(i32, i32, bool),
    /// `:nth-child(An+B of S)`, counting only siblings that match `S`.
    NthChildOf(i32, i32, bool, Vec<ComplexSelector>),
    Is(Vec<ComplexSelector>),
    Where(Vec<ComplexSelector>),
    Not(Vec<ComplexSelector>),
    /// `:has()` relative selectors, anchored at the subject element.
    Has(Vec<(Combinator, ComplexSelector)>),
    NthOfType(i32, i32, bool),
    Lang(String),
    Optional,
    ReadOnly(bool),
    PlaceholderShown,
    /// A pseudo-class that is valid but never matches here (`:visited`).
    Never,
    /// A pseudo-element (`::before`); never matches the originating element.
    PseudoElement(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    Adjacent,
    Sibling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundSelector {
    pub components: Vec<SimpleSelector>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexSelector {
    pub items: Vec<(CompoundSelector, Option<Combinator>)>,
}
