package org.acme.library.infra;

import org.acme.library.domain.Loan;

public abstract class LoanRepository {
    public abstract void save(Loan loan);
}
