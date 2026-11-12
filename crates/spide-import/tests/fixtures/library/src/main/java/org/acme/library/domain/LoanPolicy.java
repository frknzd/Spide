package org.acme.library.domain;

public interface LoanPolicy {
    int fineFor(int daysLate);
    default boolean allowsRenewal() { return true; }
}
