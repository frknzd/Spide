package org.acme.library.domain;

public class StandardPolicy implements LoanPolicy {
    @Override
    public int fineFor(int daysLate) { return daysLate * 10; }
}
