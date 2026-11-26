// SPDX-License-Identifier: MIT
#pragma once

#include <QList>
#include <QString>

/// A query as search reads it: space-separated terms, each fuzzy (its letters in order, anywhere),
/// `'exact`, `^prefix`, `suffix$`, or `!excluded`; every one must hold. Case is ignored.
class Terms {
public:
    explicit Terms(const QString &query);

    bool isEmpty() const { return m_terms.isEmpty(); }
    /// Every term holds for `text` (a path).
    bool matches(const QString &text) const;
    /// How many of the terms (excluding ones) `name` matches by itself.
    int inName(const QString &name) const;
    /// How loosely `name` holds the terms it matches: 0 when each is a run of its letters, more as
    /// they spread out. Closer matches rank first.
    int looseness(const QString &name) const;

private:
    enum Kind { Fuzzy, Exact, Prefix, Suffix, Not };
    struct Term {
        Kind kind;
        QString text;
    };
    static bool holds(const Term &term, const QString &text);
    QList<Term> m_terms;
};
