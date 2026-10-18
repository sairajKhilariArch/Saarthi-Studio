package com.saarthi.studio;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/** Builds parameterized SQL for message search and conversation-organization filters. */
final class ConversationSearch {
    private static final String SELECT =
            "SELECT c._id,c.title,c.updated_at,c.folder,c.tags,c.is_pinned,c.is_favorite FROM conversations c ";
    private static final String SEARCH_CONDITION =
            "(instr(lower(c.title), lower(?)) > 0 OR instr(lower(c.folder), lower(?)) > 0 OR " +
            "instr(lower(c.tags), lower(?)) > 0 OR EXISTS (SELECT 1 FROM messages m " +
            "WHERE m.conversation_id=c._id AND instr(lower(m.content), lower(?)) > 0))";
    private static final String ORDER =
            " ORDER BY c.is_pinned DESC,c.is_favorite DESC,c.updated_at DESC,c._id DESC LIMIT ?";

    private ConversationSearch() { }

    static Query fromText(String query) { return fromTextAndFilter(query, "All conversations"); }

    static Query fromTextAndFilter(String query, String filter) {
        String term = query == null ? "" : query.trim();
        String safeFilter = filter == null || filter.trim().isEmpty() ? "All conversations" : filter.trim();
        List<String> conditions = new ArrayList<>();
        List<String> args = new ArrayList<>();
        if (!term.isEmpty()) {
            conditions.add(SEARCH_CONDITION);
            for (int i = 0; i < 4; i++) args.add(term);
        }

        if ("Pinned only".equals(safeFilter)) {
            conditions.add("c.is_pinned=1");
        } else if ("Favorites only".equals(safeFilter)) {
            conditions.add("c.is_favorite=1");
        } else if (safeFilter.startsWith("Folder: ")) {
            conditions.add("c.folder=?");
            args.add(safeFilter.substring("Folder: ".length()));
        } else if (safeFilter.startsWith("Tag: ")) {
            conditions.add("instr(',' || lower(c.tags) || ',', ',' || lower(?) || ',') > 0");
            args.add(safeFilter.substring("Tag: ".length()));
        }

        StringBuilder sql = new StringBuilder(SELECT);
        if (!conditions.isEmpty()) sql.append(" WHERE ").append(String.join(" AND ", conditions));
        sql.append(ORDER);
        return new Query(term, safeFilter, sql.toString(), args);
    }

    static String matchingPreview(String content, String query) {
        String term = query == null ? "" : query.trim();
        if (term.isEmpty() || content == null) return "";
        String value = content.replaceAll("\\s+", " ").trim();
        int index = value.toLowerCase(Locale.ROOT).indexOf(term.toLowerCase(Locale.ROOT));
        if (index < 0) return "";
        if (index > 44) value = "…" + value.substring(index - 40);
        return value.length() > 90 ? value.substring(0, 87) + "…" : value;
    }

    static final class Query {
        private final String term;
        private final String filter;
        private final String sql;
        private final List<String> args;
        Query(String term, String filter, String sql, List<String> args) {
            this.term = term; this.filter = filter; this.sql = sql; this.args = new ArrayList<>(args);
        }
        boolean isEmpty() { return term.isEmpty(); }
        boolean isUnfiltered() { return term.isEmpty() && "All conversations".equals(filter); }
        String term() { return term; }
        String filter() { return filter; }
        String sql() { return sql; }
        String[] arguments(int limit) {
            List<String> all = new ArrayList<>(args);
            all.add(String.valueOf(Math.max(1, Math.min(limit, 500))));
            return all.toArray(new String[0]);
        }
    }
}
