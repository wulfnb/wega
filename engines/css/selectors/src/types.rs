//! Selector representation.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleSelector {
    Universal,
    Type(String),
    Class(String),
    Id(String),
    AttrPresent(String),
    AttrEquals(String, String),
    AttrMatch(String, String, char, bool),
    Root,
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
