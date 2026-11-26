// SPDX-License-Identifier: MIT
#include "Terms.h"

#include <QStringList>

Terms::Terms(const QString &query) {
    for (const QString &word : query.split(u' ', Qt::SkipEmptyParts)) {
        const QString lower = word.toLower();
        if (lower.size() > 1 && lower.startsWith(u'!')) {
            m_terms.append({Not, lower.mid(1)});
        } else if (lower.size() > 1 && lower.startsWith(u'\'')) {
            m_terms.append({Exact, lower.mid(1)});
        } else if (lower.size() > 1 && lower.startsWith(u'^')) {
            m_terms.append({Prefix, lower.mid(1)});
        } else if (lower.size() > 1 && lower.endsWith(u'$')) {
            m_terms.append({Suffix, lower.chopped(1)});
        } else {
            m_terms.append({Fuzzy, lower});
        }
    }
}

bool Terms::holds(const Term &term, const QString &text) {
    switch (term.kind) {
    case Exact:
        return text.contains(term.text);
    case Not:
        return !text.contains(term.text);
    case Prefix:
        return text.startsWith(term.text);
    case Suffix:
        return text.endsWith(term.text);
    case Fuzzy: {
        qsizetype at = 0;
        for (const QChar c : term.text) {
            at = text.indexOf(c, at);
            if (at < 0) {
                return false;
            }
            ++at;
        }
        return true;
    }
    }
    return false;
}

bool Terms::matches(const QString &text) const {
    const QString lower = text.toLower();
    return std::all_of(m_terms.cbegin(), m_terms.cend(), [&](const Term &term) { return holds(term, lower); });
}

int Terms::looseness(const QString &name) const {
    const QString lower = name.toLower();
    int loose = 0;
    for (const Term &term : m_terms) {
        if (term.kind != Fuzzy || term.text.isEmpty() || lower.contains(term.text)) {
            continue;
        }
        // The tightest span holding the letters in order, from each place the first one is.
        int best = -1;
        for (qsizetype start = lower.indexOf(term.text.front()); start >= 0; start = lower.indexOf(term.text.front(), start + 1)) {
            qsizetype at = start;
            bool held = true;
            for (const QChar c : term.text) {
                at = lower.indexOf(c, at);
                if (at < 0) {
                    held = false;
                    break;
                }
                ++at;
            }
            if (!held) {
                break;
            }
            const int span = int(at - start - term.text.size()) + 1;
            if (best < 0 || span < best) {
                best = span;
            }
        }
        loose += best < 0 ? 1000 : best;
    }
    return loose;
}

int Terms::inName(const QString &name) const {
    const QString lower = name.toLower();
    // A prefix or suffix term counts when it holds for the name itself.
    return int(std::count_if(m_terms.cbegin(), m_terms.cend(), [&](const Term &term) { return term.kind != Not && holds(term, lower); }));
}
