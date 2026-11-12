package org.acme.library.infra;

public record AuditLog(String target) {
    public void record(String what) { }
}
