let meta = #{
    name: "factory-cycle",
    description: "One factory verify cycle. Not the outer loop.",
    phases: [
        #{ title: "Verify", detail: "corpus hunter and skeptic, read-only" },
    ],
};

let report_schema = #{
    "type": "object",
    "required": ["head", "files", "tests", "review", "next"],
    "properties": #{
        "head": #{ "type": "string" },
        "files": #{ "type": "string" },
        "tests": #{ "type": "string" },
        "review": #{ "type": "string" },
        "next": #{ "type": "string" },
    },
};

let job_id = if args == () { () } else { args.job_id };
let work_source = if args == () { () } else { args.work_source };
let focus = if args == () { () } else { args.focus };
if job_id == () {
    pause("verification", "Pass args.job_id and args.work_source.");
}

phase("Verify");
let jobs = [];
jobs.push(#{
    prompt: "Read-only corpus hunter. job_id=" + job_id + " work_source=" + work_source
        + " focus=" + focus + ". Use grep and read_file on tests/corpus and the last git diff. "
        + "Do not edit. Report files that Bline would accept and this crate would miss. "
        + "Empty list is valid only after you searched.",
    label: "corpus-hunter",
    capability_mode: "read-only",
    output_schema: report_schema,
});
jobs.push(#{
    prompt: "Read-only skeptic. job_id=" + job_id + " work_source=" + work_source
        + ". Use grep and read_file. Do not edit. Confirm or refute the last child TESTS line. "
        + "Set review to 0 must-fix only after you read the named test.",
    label: "skeptic",
    capability_mode: "read-only",
    output_schema: report_schema,
});
let results = parallel(jobs);

let hunter = if results.len() > 0 { results[0] } else { () };
let skeptic = if results.len() > 1 { results[1] } else { () };
if hunter == () || !hunter.success || skeptic == () || !skeptic.success {
    complete(#{
        summary: "verify failed closed",
        next: "next-cycle",
    });
}
complete(#{
    summary: "one factory cycle verify",
    hunter: hunter.output,
    skeptic: skeptic.output,
});
