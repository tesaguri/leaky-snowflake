#!/usr/bin/env ruby
# frozen_string_literal: true

# = stream-to-list.rb
#
# Reads Tweets from the Sample Stream and adds their authors to a List.

list = Integer(ARGV[0])

require 'set'

require 'twitter'
require 'twurl'

MAX_LIST_LENGTH = 5000

rest = Twitter::REST::Client.new do |client|
  twurlrc = Twurl::RCFile.new
  (username, consumer_key) = twurlrc.default_profile
  profile = twurlrc.profiles.fetch(username).fetch(consumer_key)
  client.consumer_key = profile.fetch('consumer_key')
  client.consumer_secret = profile.fetch('consumer_secret')
  client.access_token = profile.fetch('token')
  client.access_token_secret = profile.fetch('secret')
end

users = rest.list_members(list, count: MAX_LIST_LENGTH).each_with_object(Set.new) do |user, set|
  set.add user.id
end

def finish
  puts "Collected #{MAX_LIST_LENGTH} users"
  exit
end

finish if users.length >= MAX_LIST_LENGTH

stack = []
current_user = nil

adder = Thread.new do
  until users.length >= MAX_LIST_LENGTH
    sleep until current_user = stack.pop

    begin
      rest.add_list_member(list, current_user)
      puts "+#{current_user}"
      users.add current_user
    rescue Twitter::Error::TooManyRequests => e
      wait = e.rate_limit.reset_in
      STDERR.puts "Will retry in #{wait} seconds..."
      loop do
        sleep wait
        break if (wait = e.rate_limit.reset_in) <= 0
      end
      # Dropping the current user as we're going to have a bunch of new users anyway
      next
    rescue Twitter::Error => e
      STDERR.puts "Unable to add user `#{current_user}` to the List: #{e.class.name} (#{e.message})"
    end
  end

  finish
end
adder.abort_on_exception = true

stream = Twitter::Streaming::Client.new do |client|
  client.consumer_key = rest.consumer_key
  client.consumer_secret = rest.consumer_secret
  client.access_token = rest.access_token
  client.access_token_secret = rest.access_token_secret
end
stream.sample do |t|
  next unless t.is_a? Twitter::Tweet
  unless [users, stack, [current_user]].any? {|enum| enum.include? t.user.id }
    stack.push t.user.id
    if stack.length > MAX_LIST_LENGTH - users.length
      # Prevent the stack from having excessive # of elements while sleeping
      stack.shift
    end
    adder.run
  end
end
