# frozen_string_literal: true

require "bundler/gem_tasks"
require "rspec/core/rake_task"

RSpec::Core::RakeTask.new(:spec)

require "standard/rake"

require "rb_sys/extensiontask"

task build: :compile

GEMSPEC = Gem::Specification.load("ship_compliant_v1_rb.gemspec")

RbSys::ExtensionTask.new("ship_compliant_v1_rb", GEMSPEC) do |ext|
  ext.lib_dir = "lib/ship_compliant_v1_rb"
end

task default: %i[compile spec standard]
