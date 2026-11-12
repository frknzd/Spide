package org.acme.library.domain;

public enum LoanType {
    NORMAL, SHORT;

    private int days;

    public int days() { return days; }
}
