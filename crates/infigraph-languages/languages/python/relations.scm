; Python relationship extraction queries for terragraph

; Function/method calls
(call
  function: (identifier) @call.func) @call.site

; Method calls on objects (obj.method())
(call
  function: (attribute
    object: (_) @call.receiver
    attribute: (identifier) @call.func)) @call.site

; Import statements: import foo
(import_statement
  name: (dotted_name) @import.module)

; From imports: from foo import bar
(import_from_statement
  module_name: (dotted_name) @import.module)

; Relative from-imports: from .foo import bar / from ..pkg.foo import bar.
; module_name is (relative_import (import_prefix) (dotted_name)?) here, not a
; bare dotted_name, so the pattern above never matches these on its own.
(import_from_statement
  module_name: (relative_import
    (dotted_name) @import.module))

; Class inheritance: class Foo(Bar). superclasses can be plain identifiers, dotted
; names (pkg.Bar), or subscripted generics (Generic[T]); matching the "expression"
; supertype (rather than a bare wildcard) correctly excludes keyword_argument nodes
; like metaclass=Meta, which are NOT base classes.
(class_definition
  name: (identifier) @inherit.child
  superclasses: (argument_list
    (expression) @inherit.parent))

; Decorator on a function: @decorator def func()
(decorated_definition
  (decorator (identifier) @decorates.target)
  definition: (function_definition
    name: (identifier) @decorates.source))

; Decorator on a class: @decorator class Foo
(decorated_definition
  (decorator (identifier) @decorates.target)
  definition: (class_definition
    name: (identifier) @decorates.source))
