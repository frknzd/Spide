package org.acme.library.domain;

import org.acme.library.infra.AuditLog;

/** Loan knows its member (cycle with Member) and writes to the audit log (layer violation). */
public class Loan {
    private Member member;
    private Book book;
    private LoanPolicy policy;
    private LoanType type;
    private AuditLog audit;

    public int fine(int daysLate) {
        audit.record("fine");
        return policy.fineFor(daysLate);
    }

    public Member getMember() { return member; }
}
