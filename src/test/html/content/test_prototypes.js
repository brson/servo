is(window.document.documentElement instanceof Node, true);
is(window.document.documentElement instanceof Element, true);
is(window.document.documentElement instanceof HTMLElement, true);
is(window.document.documentElement instanceof HTMLHtmlElement, true);
is(window.document instanceof Document, true);
is(window.document instanceof HTMLDocument, true);
is(window.document.documentElement.tagName, "HTML");
is(window.document.getElementsByTagName('foo-á')[0] instanceof HTMLUnknownElement, true);
is(window.document.getElementsByTagName('foo-á')[0].tagName, "FOO-á");
finish();
