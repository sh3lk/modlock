//! modlock-macros holds the macros of the modlock crate, which exports them:
//! a mod names them as modlock's own.
//!
//! The crate has no dependencies, so a mod's build pays for no parser of
//! Rust: ui! reads only the tags, and hands each value between braces to the
//! compiler as it stands.

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use std::iter::Peekable;

/// ui! writes an interface as tags. Each tag is an element of modlock::ui or
/// a component, each attribute one of its methods, and what stands between a
/// tag and its closing tag are its children:
///
/// ```ignore
/// show(player, ui! {
///     <panel style={{ flow: Flow::Down, padding: 16, background: "#000000cc" }}>
///         <label text={format!("Clicks: {clicks}")} />
///         <button on_press={Self::click}>"Click"</button>
///         {rows}
///     </panel>
/// });
/// ```
///
/// An attribute's value is a literal or any expression between braces, and an
/// attribute without a value is true. Two pairs of braces hold the members of
/// a style, or of whatever the attribute takes, each set from a value of any
/// type that fits it: a number of pixels for a size, text for a color.
///
/// A child is a tag, text between quotes, which is a label, or any expression
/// between braces that an element may hold: an element or a list of them.
///
/// A tag that names no element of modlock::ui is a component: a function
/// marked #[component], by its name or its path, such as <hud::banner>.
///
/// The macro's value is the Element of its one tag, so that tag is not a
/// component that may draw nothing; such a component is a child.
#[proc_macro]
pub fn ui(input: TokenStream) -> TokenStream {
    let mut tags = Tags {
        rest: input.into_iter().peekable(),
        last: Span::call_site(),
    };
    let built = tags.tag().and_then(|tag| match tags.rest.next() {
        Some(extra) => Err(Refusal {
            at: extra.span(),
            why: "ui! takes one tag; put the rest inside it",
        }),
        None => Ok(tag),
    });
    match built {
        Ok(tag) => call("::modlock::ui::Element::from", tag),
        Err(refusal) => refusal.tokens(),
    }
}

/// ELEMENTS are the elements of modlock::ui, which a tag names without a
/// path. Any other tag is a component, named as the mod's code names it.
const ELEMENTS: [&str; 15] = [
    "panel",
    "label",
    "image",
    "button",
    "hero",
    "avatar",
    "entry",
    "slider",
    "toggle",
    "dropdown",
    "layout",
    "menu",
    "menu_close",
    "document_input",
    "document_output",
];

/// spelled returns the tokens as text without spaces, to compare two paths.
fn spelled(tokens: &TokenStream) -> String {
    tokens.to_string().split_whitespace().collect()
}

/// Refusal is why the macro could not read its tags, and where.
struct Refusal {
    at: Span,
    why: &'static str,
}

impl Refusal {
    /// tokens returns the compile error that reports the refusal at its
    /// place.
    fn tokens(&self) -> TokenStream {
        let mut message = TokenTree::Literal(Literal::string(self.why));
        message.set_span(self.at);
        let mut bang = TokenTree::Punct(Punct::new('!', Spacing::Alone));
        bang.set_span(self.at);
        let mut body = TokenTree::Group(Group::new(Delimiter::Parenthesis, message.into()));
        body.set_span(self.at);
        let name = TokenTree::Ident(Ident::new("compile_error", self.at));
        TokenStream::from_iter([name, bang, body])
    }
}

/// Tags reads the tags of one ui!.
struct Tags {
    rest: Peekable<proc_macro::token_stream::IntoIter>,
    /// last is where the last token read stood, to report an early end at.
    last: Span,
}

impl Tags {
    /// next reads one token.
    fn next(&mut self, why: &'static str) -> Result<TokenTree, Refusal> {
        match self.rest.next() {
            Some(token) => {
                self.last = token.span();
                Ok(token)
            }
            None => Err(Refusal { at: self.last, why }),
        }
    }

    /// mark reads the mark c, and refuses anything else.
    fn mark(&mut self, c: char, why: &'static str) -> Result<(), Refusal> {
        match self.next(why)? {
            TokenTree::Punct(punct) if punct.as_char() == c => Ok(()),
            other => Err(Refusal {
                at: other.span(),
                why,
            }),
        }
    }

    /// marked reports whether the next token is the mark c.
    fn marked(&mut self, c: char) -> bool {
        matches!(self.rest.peek(), Some(TokenTree::Punct(punct)) if punct.as_char() == c)
    }

    /// name reads a tag's or an attribute's name.
    fn name(&mut self, why: &'static str) -> Result<Ident, Refusal> {
        match self.next(why)? {
            TokenTree::Ident(name) => Ok(name),
            other => Err(Refusal {
                at: other.span(),
                why,
            }),
        }
    }

    /// path reads a tag's name, which may be a path, such as hud::banner, and
    /// returns its first name with the whole of it.
    fn path(&mut self, why: &'static str) -> Result<(Ident, TokenStream), Refusal> {
        let first = self.name(why)?;
        let mut path = TokenStream::from(TokenTree::Ident(first.clone()));
        while self.marked(':') {
            path.extend([self.next(why)?, self.next(why)?]);
            path.extend([TokenTree::Ident(self.name(why)?)]);
        }
        Ok((first, path))
    }

    /// tag reads one tag with its attributes and children, and returns the
    /// expression that builds its element.
    fn tag(&mut self) -> Result<TokenStream, Refusal> {
        // The tag's name is the function that makes its element: one of the
        // library's elements, or a component by its name or path.
        self.mark('<', "a tag starts with <")?;
        let (name, path) = self.path("a tag is named after an element, such as <panel>")?;
        let mut built = TokenStream::new();
        if ELEMENTS.contains(&spelled(&path).as_str()) {
            built.extend(tokens("::modlock::ui::"));
        }
        built.extend(path.clone());
        built.extend([group(Delimiter::Parenthesis, TokenStream::new())]);

        // Each attribute is a method of the element.
        loop {
            if self.marked('/') {
                self.next("")?;
                self.mark('>', "a tag without children ends with />")?;
                return Ok(built);
            }
            if self.marked('>') {
                self.next("")?;
                break;
            }
            let attribute = self.name("an attribute is a name, then = and its value")?;
            let value = if self.marked('=') {
                self.next("")?;
                self.value(&attribute)?
            } else {
                tokens("true")
            };
            built.extend(method(attribute, value));
        }

        // Each child is given to the element until its closing tag.
        loop {
            // A tag that holds no children refuses the child where it stands.
            let at = self.rest.peek().map_or(name.span(), TokenTree::span);
            let child = match self.rest.peek() {
                Some(TokenTree::Punct(punct)) if punct.as_char() == '<' => {
                    let mut ahead = self.rest.clone();
                    ahead.next();
                    match ahead.next() {
                        Some(TokenTree::Punct(slash)) if slash.as_char() == '/' => break,
                        _ => self.tag()?,
                    }
                }
                Some(TokenTree::Group(braces)) if braces.delimiter() == Delimiter::Brace => {
                    let held = braces.stream();
                    self.next("")?;
                    held
                }
                Some(TokenTree::Literal(_)) => {
                    let text = self.next("")?;
                    call("::modlock::ui::text", text.into())
                }
                Some(other) => {
                    let why =
                        "a child is a tag, text between quotes, or an expression between braces";
                    return Err(Refusal {
                        at: other.span(),
                        why,
                    });
                }
                None => {
                    let why = "the tag is not closed; close it as </name>, or end it with />";
                    return Err(Refusal {
                        at: name.span(),
                        why,
                    });
                }
            };
            built.extend(method(Ident::new("child", at), child));
        }

        // The closing tag names the tag it closes.
        self.mark('<', "")?;
        self.mark('/', "")?;
        let (closed, closing) = self.path("a closing tag names its tag, as </panel>")?;
        if spelled(&closing) != spelled(&path) {
            return Err(Refusal {
                at: closed.span(),
                why: "the closing tag names another tag than the one open",
            });
        }
        self.mark('>', "a closing tag ends with >")?;
        Ok(built)
    }

    /// value reads an attribute's value: a literal, an expression between
    /// braces, or members between two pairs of braces.
    fn value(&mut self, attribute: &Ident) -> Result<TokenStream, Refusal> {
        let why = "an attribute's value is a literal or an expression between braces";
        match self.next(why)? {
            TokenTree::Literal(literal) => Ok(TokenTree::Literal(literal).into()),
            TokenTree::Group(braces) if braces.delimiter() == Delimiter::Brace => {
                match members(&braces) {
                    Some(inner) => filled(attribute, inner),
                    None => Ok(braces.stream()),
                }
            }
            other => Err(Refusal {
                at: other.span(),
                why,
            }),
        }
    }
}

/// members returns the tokens between the inner braces of a value written
/// between two pairs, when they are members: empty, or opening with a name
/// and a colon. Anything else between two pairs of braces is a block.
fn members(braces: &Group) -> Option<TokenStream> {
    let mut held = braces.stream().into_iter();
    let inner = match (held.next(), held.next()) {
        (Some(TokenTree::Group(inner)), None) if inner.delimiter() == Delimiter::Brace => inner,
        _ => return None,
    };
    let mut opening = inner.stream().into_iter();
    match (opening.next(), opening.next(), opening.next()) {
        (None, ..) => Some(inner.stream()),
        (Some(TokenTree::Ident(_)), Some(TokenTree::Punct(colon)), after)
            if colon.as_char() == ':'
                && !matches!(&after, Some(TokenTree::Punct(next)) if next.as_char() == ':') =>
        {
            Some(inner.stream())
        }
        _ => None,
    }
}

/// MEMBERED maps each attribute that takes members between two pairs of
/// braces to the type it takes.
const MEMBERED: [(&str, &str); 6] = [
    ("style", "::modlock::Style"),
    ("hover", "::modlock::Style"),
    ("enter", "::modlock::Style"),
    ("exit", "::modlock::Style"),
    ("cycle", "::modlock::Cycle"),
    ("clock", "::modlock::Clock"),
];

/// filled returns the expression that makes the value attribute takes, with
/// each of members set: a name, a colon and a value, separated by commas.
/// Every other member is its default.
fn filled(attribute: &Ident, members: TokenStream) -> Result<TokenStream, Refusal> {
    // Find the type the attribute takes.
    let named = attribute.to_string();
    let Some((_, held)) = MEMBERED.iter().find(|(name, _)| *name == named) else {
        let why = "this attribute takes no members between two pairs of braces; \
                   style, hover, enter, exit, cycle and clock do";
        return Err(Refusal {
            at: attribute.span(),
            why,
        });
    };

    // Set each member from its value, as whatever stands for the member.
    let mut body = TokenStream::new();
    let mut rest = members.into_iter().peekable();
    while rest.peek().is_some() {
        let why = "a member is a name, a colon and its value";
        let name = match rest.next() {
            Some(TokenTree::Ident(name)) => name,
            Some(other) => {
                return Err(Refusal {
                    at: other.span(),
                    why,
                })
            }
            None => break,
        };
        match rest.next() {
            Some(TokenTree::Punct(colon)) if colon.as_char() == ':' => {}
            _ => {
                return Err(Refusal {
                    at: name.span(),
                    why,
                })
            }
        }

        // The value runs to the next comma outside any brackets.
        let mut value = TokenStream::new();
        for token in rest.by_ref() {
            if matches!(&token, TokenTree::Punct(comma) if comma.as_char() == ',') {
                break;
            }
            value.extend([token]);
        }
        if value.is_empty() {
            return Err(Refusal {
                at: name.span(),
                why,
            });
        }
        body.extend([TokenTree::Ident(name)]);
        body.extend(tokens(":"));
        body.extend(call("::modlock::ui::Set::set_as", value));
        body.extend(tokens(","));
    }
    body.extend(tokens("..::core::default::Default::default()"));
    let mut built = tokens(held);
    built.extend([group(Delimiter::Brace, body)]);
    Ok(built)
}

/// tokens returns the tokens of code the macro itself writes.
fn tokens(code: &str) -> TokenStream {
    code.parse().expect("the macro writes code that reads")
}

/// group returns held between a pair of brackets.
fn group(delimiter: Delimiter, held: TokenStream) -> TokenTree {
    TokenTree::Group(Group::new(delimiter, held))
}

/// call returns the call of the function at path with argument.
fn call(path: &str, argument: TokenStream) -> TokenStream {
    let mut built = tokens(path);
    built.extend([group(Delimiter::Parenthesis, argument)]);
    built
}

/// method returns the call of the method name with argument, to follow the
/// value it is called on.
fn method(name: Ident, argument: TokenStream) -> TokenStream {
    let dot = TokenTree::Punct(Punct::new('.', Spacing::Alone));
    TokenStream::from_iter([
        dot,
        TokenTree::Ident(name),
        group(Delimiter::Parenthesis, argument),
    ])
}

/// component makes a function a component: an element built from others,
/// which a mod writes as a tag. The function's parameters are the tag's
/// attributes, and it returns what the tag draws: an Element, or an
/// Option<Element>, written so, to draw nothing for None:
///
/// ```ignore
/// #[component]
/// fn row(name: String, points: u32, own: bool) -> Element {
///     ui! {
///         <panel style={{ flow: Flow::Right }}>
///             <label text={name} />
///             <label text={points.to_string()} />
///         </panel>
///     }
/// }
///
/// ui! { <row name="Ann" points={3} own /> }
/// ```
///
/// The macro keeps the function's body and writes around it what a tag needs:
/// a function of the same name that takes nothing, and a method for each
/// parameter that sets it. So the component is plain code too:
/// row().name("Ann").points(3).own(true).
///
/// A parameter the tag leaves out is its type's default, so each type has
/// one; an Option takes its value alone. A parameter named children, a
/// Vec<Element>, takes what stands between the tag and its closing tag. One of
/// type Option<Press> takes a handler as a button's on_press does. Every
/// component takes a key, as every element does.
///
/// A component that returns an Option<Element> is a child of an element or
/// what show takes. Where it draws nothing it holds its place, so the
/// children after it keep their nodes when it draws again. It is no Element
/// by itself: neither the one tag of a ui! nor one of a list of components.
#[proc_macro_attribute]
pub fn component(_arguments: TokenStream, item: TokenStream) -> TokenStream {
    match Component::read(item) {
        Ok(component) => component.written(),
        Err(refusal) => refusal.tokens(),
    }
}

/// Property is one parameter of a component's function.
struct Property {
    /// notes are the parameter's attributes, its doc comment among them.
    notes: TokenStream,
    /// binding is the parameter as the function's body names it, with mut
    /// when it has it.
    binding: TokenStream,
    name: Ident,
    kind: TokenStream,
}

/// Component is a function marked #[component], taken apart.
struct Component {
    notes: TokenStream,
    reach: TokenStream,
    name: Ident,
    properties: Vec<Property>,
    returns: TokenStream,
    body: TokenTree,
}

impl Component {
    /// read takes the function apart.
    fn read(item: TokenStream) -> Result<Component, Refusal> {
        let why = "#[component] marks a function: fn name(property: Type) -> Element { ... }";
        let mut rest = item.into_iter().peekable();
        let mut last = Span::call_site();
        let mut next = |rest: &mut Peekable<proc_macro::token_stream::IntoIter>| match rest.next() {
            Some(token) => {
                last = token.span();
                Ok(token)
            }
            None => Err(Refusal { at: last, why }),
        };

        // The attributes and the reach come before fn.
        let mut notes = TokenStream::new();
        let mut reach = TokenStream::new();
        let name = loop {
            match next(&mut rest)? {
                TokenTree::Punct(hash) if hash.as_char() == '#' => {
                    notes.extend([TokenTree::Punct(hash), next(&mut rest)?]);
                }
                TokenTree::Ident(word) if word.to_string() == "fn" => match next(&mut rest)? {
                    TokenTree::Ident(name) => break name,
                    other => {
                        return Err(Refusal {
                            at: other.span(),
                            why,
                        })
                    }
                },
                other => reach.extend([other]),
            }
        };

        // The parameters stand between the parentheses.
        let parameters = match next(&mut rest)? {
            TokenTree::Group(group) if group.delimiter() == Delimiter::Parenthesis => group,
            other => {
                let why = "a component's function takes no type parameters";
                return Err(Refusal {
                    at: other.span(),
                    why,
                });
            }
        };
        let properties = properties(parameters.stream())?;

        // What it returns runs from the arrow to the body.
        let mut returns = TokenStream::new();
        let body = loop {
            match next(&mut rest)? {
                TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
                    break TokenTree::Group(group)
                }
                other => returns.extend([other]),
            }
        };
        if returns.is_empty() {
            let why = "a component's function returns what it draws: -> Element, \
                       or -> Option<Element>";
            return Err(Refusal {
                at: name.span(),
                why,
            });
        }
        Ok(Component {
            notes,
            reach,
            name,
            properties,
            returns,
            body,
        })
    }

    /// written returns the component as the compiler reads it: the function
    /// that takes nothing, the type it returns with a method for each
    /// property, and the type's place among the elements.
    fn written(self) -> TokenStream {
        let Component {
            notes,
            reach,
            name,
            properties,
            returns,
            body,
        } = self;
        let kind = TokenStream::from(TokenTree::Ident(Ident::new(
            &camel(&name.to_string()),
            name.span(),
        )));
        let named = TokenStream::from(TokenTree::Ident(name.clone()));

        // The type holds each property, and the function makes it with each
        // as its default.
        let mut members = TokenStream::new();
        let mut defaults = TokenStream::new();
        let mut bindings = TokenStream::new();
        let mut methods = TokenStream::new();
        for property in &properties {
            let own = TokenStream::from(TokenTree::Ident(property.name.clone()));
            members.extend(templated(
                "__P0: __P1,",
                &[own.clone(), property.kind.clone()],
            ));
            defaults.extend(templated(
                "__P0: ::core::default::Default::default(),",
                std::slice::from_ref(&own),
            ));
            bindings.extend(property.binding.clone());
            bindings.extend(tokens(","));
            methods.extend(property.notes.clone());
            methods.extend(property.method(&reach));
        }

        // An element stands for a component, under the component's key. A
        // component that may draw nothing is a list of one element or none,
        // and so is no element itself.
        let place = match optional(&returns) {
            false => {
                "impl ::core::convert::From<__P0> for ::modlock::ui::Element {
                    fn from(component: __P0) -> Self {
                        let key = component.__key.clone();
                        let drawn: ::modlock::ui::Element = ::core::convert::Into::into(component.__render());
                        match key.is_empty() {
                            true => drawn,
                            false => drawn.key(key),
                        }
                    }
                }

                impl ::modlock::ui::Children for __P0 {
                    fn slot(self) -> ::modlock::ui::SlotOf {
                        ::modlock::ui::slot_of(::core::convert::Into::into(self))
                    }
                }"
            }
            true => {
                "impl ::modlock::ui::Children for __P0 {
                    fn slot(self) -> ::modlock::ui::SlotOf {
                        let key = self.__key.clone();
                        let drawn: ::core::option::Option<::modlock::ui::Element> =
                            ::core::option::Option::map(self.__render(), ::core::convert::Into::into);
                        ::modlock::ui::Children::slot(::core::option::Option::map(drawn, |drawn| {
                            match key.is_empty() {
                                true => drawn,
                                false => drawn.key(key),
                            }
                        }))
                    }
                }"
            }
        };
        let place = templated(place, std::slice::from_ref(&kind));
        let mut written = notes;
        written.extend(templated(
            "__P0 fn __P1() -> __P2 {
                __P2 { __key: ::std::string::String::new(), __P3 }
            }

            #[derive(Clone)]
            __P0 struct __P2 { __key: ::std::string::String, __P4 }

            impl __P2 {
                /// key keeps the component's node in a list while the list
                /// changes.
                __P0 fn key(mut self, key: impl ::std::string::ToString) -> Self {
                    self.__key = key.to_string();
                    self
                }

                __P5

                fn __render(self) __P6 {
                    let Self { __key: _, __P7 } = self;
                    __P8
                }
            }",
            &[
                reach,
                named,
                kind,
                defaults,
                members,
                methods,
                returns,
                bindings,
                body.into(),
            ],
        ));
        written.extend(place);
        written
    }
}

impl Property {
    /// method returns the method that sets the property, as far as reach lets
    /// it be called from.
    fn method(&self, reach: &TokenStream) -> TokenStream {
        let name = TokenStream::from(TokenTree::Ident(self.name.clone()));
        let kind = spelled(&self.kind);

        // The children are added to, and a handler is any function that
        // takes the mod and the player.
        if self.name.to_string() == "children" {
            return templated(
                "__P0 fn child(mut self, children: impl ::modlock::ui::Children) -> Self {
                    self.children.extend(::modlock::ui::elements(children));
                    self
                }",
                std::slice::from_ref(reach),
            );
        }
        if kind.starts_with("Option<") && kind.ends_with("Press>") {
            return templated(
                "__P0 fn __P1<M: 'static>(mut self, handler: impl ::modlock::ui::Pressed<M>) -> Self {
                    self.__P1 = ::core::option::Option::Some(::modlock::ui::Pressed::press(handler));
                    self
                }",
                &[reach.clone(), name],
            );
        }
        templated(
            "__P0 fn __P1(mut self, __P1: impl ::modlock::ui::Set<__P2>) -> Self {
                self.__P1 = ::modlock::ui::Set::set_as(__P1);
                self
            }",
            &[reach.clone(), name, self.kind.clone()],
        )
    }
}

/// properties reads a function's parameters: each a name, a colon and a type,
/// separated by commas.
fn properties(parameters: TokenStream) -> Result<Vec<Property>, Refusal> {
    let why = "a property is a name, a colon and its type";
    let mut read = Vec::new();
    let mut rest = parameters.into_iter().peekable();
    while rest.peek().is_some() {
        // The attributes, then the name, which may be mut.
        let mut notes = TokenStream::new();
        let mut binding = TokenStream::new();
        let name = loop {
            match rest.next() {
                Some(TokenTree::Punct(hash)) if hash.as_char() == '#' => {
                    notes.extend([TokenTree::Punct(hash)]);
                    notes.extend(rest.next());
                }
                Some(TokenTree::Ident(word)) if word.to_string() == "mut" => {
                    binding.extend([TokenTree::Ident(word)]);
                }
                Some(TokenTree::Ident(name)) => break name,
                Some(other) => {
                    return Err(Refusal {
                        at: other.span(),
                        why,
                    })
                }
                None => return Ok(read),
            }
        };
        binding.extend([TokenTree::Ident(name.clone())]);
        if name.to_string() == "key" {
            let why = "every component takes a key already; name the property otherwise";
            return Err(Refusal {
                at: name.span(),
                why,
            });
        }
        match rest.next() {
            Some(TokenTree::Punct(colon)) if colon.as_char() == ':' => {}
            _ => {
                return Err(Refusal {
                    at: name.span(),
                    why,
                })
            }
        }

        // The type runs to the next comma outside its angle brackets.
        let mut kind = TokenStream::new();
        let mut depth = 0;
        let mut arrow = false;
        for token in rest.by_ref() {
            if let TokenTree::Punct(punct) = &token {
                match punct.as_char() {
                    ',' if depth == 0 => break,
                    '<' => depth += 1,
                    '>' if !arrow => depth -= 1,
                    _ => {}
                }
                arrow = punct.as_char() == '-';
            } else {
                arrow = false;
            }
            kind.extend([token]);
        }
        if kind.is_empty() {
            return Err(Refusal {
                at: name.span(),
                why,
            });
        }
        read.push(Property {
            notes,
            binding,
            name,
            kind,
        });
    }
    Ok(read)
}

/// optional reports whether returns, a function's arrow and the type after it,
/// names an Option of something, by its name or its path.
fn optional(returns: &TokenStream) -> bool {
    let spelled = spelled(returns);
    let named = spelled.split_once('<').map(|(named, _)| named);
    named.and_then(|named| named.trim_start_matches("->").rsplit("::").next()) == Some("Option")
}

/// camel returns a function's name as its type's: top_strip is TopStrip.
fn camel(name: &str) -> String {
    name.split('_')
        .flat_map(|word| {
            let mut letters = word.chars();
            let first = letters.next().map(|letter| letter.to_ascii_uppercase());
            first.into_iter().chain(letters)
        })
        .collect()
}

/// templated returns the code of template with each __P and a number in it
/// replaced by the part of that number, which keeps the place it was written
/// at, so the compiler reports a mistake in it there.
fn templated(template: &str, parts: &[TokenStream]) -> TokenStream {
    fn replaced(stream: TokenStream, parts: &[TokenStream]) -> TokenStream {
        let mut out = TokenStream::new();
        for token in stream {
            match token {
                TokenTree::Ident(word) => {
                    let spelled = word.to_string();
                    match spelled
                        .strip_prefix("__P")
                        .and_then(|number| number.parse::<usize>().ok())
                    {
                        Some(number) => out.extend(parts[number].clone()),
                        None => out.extend([TokenTree::Ident(word)]),
                    }
                }
                TokenTree::Group(held) => {
                    let inner = replaced(held.stream(), parts);
                    out.extend([TokenTree::Group(Group::new(held.delimiter(), inner))]);
                }
                other => out.extend([other]),
            }
        }
        out
    }
    replaced(tokens(template), parts)
}
