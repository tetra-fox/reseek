// jsdom lays nothing out and has no scrollIntoView; lists call it to keep the focused row in view
Element.prototype.scrollIntoView = function () {};
